//! Locate the `claude` executable. A GUI process inherits a lean PATH that has
//! none of the user's shell additions, so a plain `Command::new("claude")` fails
//! for most installs. Three tiers: explicit path → login-shell lookup → the
//! well-known install locations.
use std::path::{Path, PathBuf};

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude/local/claude"),
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

/// Pure core, injectable for tests: `shell_lookup` stands in for the login
/// shell's `command -v claude`, `is_exec` for "exists and is executable".
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn() -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(p);
        if is_exec(&p) {
            return Some(p);
        }
    }
    if let Some(p) = shell_lookup() {
        if is_exec(&p) {
            return Some(p);
        }
    }
    candidates(home).into_iter().find(|c| is_exec(c))
}

/// Production entry. The login-shell lookup needs both `-l` (profile) and `-i`
/// (rc file) — without them a claude installed in ~/.local/bin stays invisible.
pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    discover_with(explicit, &home, shell_lookup, is_executable)
}

const PATH_MARKER: &str = "__NOTEMD_PATH__";

/// 一个 login+interactive 的 zsh 要把用户整份 rc 跑一遍 —— 几百毫秒起步,rc 重的
/// 机器上要好几秒。而 `discover` 是在插件的协议读循环里同步调用的:每起一次 run
/// 就把读循环按住那么久,期间这个进程的应答全都发不出去。装在哪儿这件事一个进程
/// 生命周期内不会变,查一次就够;结果仍要过 `is_exec`(见 `discover_with`),所以
/// 缓存到的路径万一被卸载了,还是会退回到候选目录,不会指着一个不存在的 claude。
///
/// 同一次探测顺手把登录 shell 的 `PATH` 也带回来:claude 自己要靠 `PATH` 去拉起
/// stdio MCP server(`npx` 之类),而 GUI 继承的那份里什么都没有。两件事合一次问,
/// 不多付一次 rc 的钱。
fn probe() -> &'static (Option<PathBuf>, Option<String>) {
    static CACHED: std::sync::OnceLock<(Option<PathBuf>, Option<String>)> =
        std::sync::OnceLock::new();
    CACHED.get_or_init(|| {
        let Ok(out) = std::process::Command::new("/bin/zsh")
            .args([
                "-lic",
                &format!(r#"command -v claude; echo "{PATH_MARKER}$PATH""#),
            ])
            .output()
        else {
            return (None, None);
        };
        parse_probe(&String::from_utf8_lossy(&out.stdout))
    })
}

/// Split the probe's stdout into the claude path and the login `PATH`.
///
/// An rc file is free to print anything it likes before the answers, so neither
/// is taken positionally: the `PATH` is the marked line, and the binary is the
/// LAST unmarked absolute path — `command -v` writes it just before the marker.
pub fn parse_probe(out: &str) -> (Option<PathBuf>, Option<String>) {
    let mut bin = None;
    let mut path = None;
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match line.strip_prefix(PATH_MARKER) {
            Some(p) if !p.is_empty() => path = Some(p.to_string()),
            Some(_) => {}
            None if line.starts_with('/') => bin = Some(PathBuf::from(line)),
            None => {}
        }
    }
    (bin, path)
}

fn shell_lookup() -> Option<PathBuf> {
    probe().0.clone()
}

/// The `PATH` a spawned claude should see. The login shell's comes first — it is
/// the one the user actually installs things into — then the usual install dirs
/// as a net for when there is no login shell to ask, then whatever this process
/// inherited, so nothing that used to resolve stops resolving.
pub fn runtime_path() -> String {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let inherited = std::env::var("PATH").unwrap_or_default();
    runtime_path_with(probe().1.as_deref(), &inherited, &home)
}

pub fn runtime_path_with(login: Option<&str>, inherited: &str, home: &Path) -> String {
    let fallbacks = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
    ];
    let mut out: Vec<String> = Vec::new();
    let entries = login
        .unwrap_or_default()
        .split(':')
        .map(str::to_string)
        .chain(fallbacks.iter().map(|p| p.to_string_lossy().into_owned()))
        .chain(inherited.split(':').map(str::to_string));
    for e in entries.filter(|e| !e.is_empty()) {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out.join(":")
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_wins() {
        let got = discover_with(
            Some("/custom/claude"),
            Path::new("/home/u"),
            || Some(PathBuf::from("/shell/claude")),
            |_| true,
        );
        assert_eq!(got, Some(PathBuf::from("/custom/claude")));
    }

    #[test]
    fn falls_back_to_shell_lookup_when_explicit_missing() {
        let got = discover_with(
            Some("/gone/claude"),
            Path::new("/home/u"),
            || Some(PathBuf::from("/shell/claude")),
            |p| p != Path::new("/gone/claude"),
        );
        assert_eq!(got, Some(PathBuf::from("/shell/claude")));
    }

    #[test]
    fn falls_back_to_candidates_when_shell_finds_nothing() {
        let home = Path::new("/home/u");
        let want = home.join(".local/bin/claude");
        let w = want.clone();
        let got = discover_with(None, home, || None, move |p| p == w);
        assert_eq!(got, Some(want));
    }

    #[test]
    fn prefers_the_claude_local_install_over_homebrew() {
        let home = Path::new("/home/u");
        let got = discover_with(None, home, || None, |_| true);
        assert_eq!(got, Some(home.join(".claude/local/claude")));
    }

    #[test]
    fn returns_none_when_nothing_is_executable() {
        assert_eq!(
            discover_with(None, Path::new("/home/u"), || None, |_| false),
            None
        );
    }

    #[test]
    fn the_probe_yields_both_the_binary_and_the_login_path() {
        let (bin, path) = parse_probe("/opt/homebrew/bin/claude\n__NOTEMD_PATH__/a/b:/c\n");
        assert_eq!(bin, Some(PathBuf::from("/opt/homebrew/bin/claude")));
        assert_eq!(path.as_deref(), Some("/a/b:/c"));
    }

    #[test]
    fn a_path_survives_a_shell_that_could_not_find_claude() {
        let (bin, path) = parse_probe("__NOTEMD_PATH__/a/b\n");
        assert_eq!(bin, None);
        assert_eq!(path.as_deref(), Some("/a/b"));
    }

    #[test]
    fn rc_file_chatter_before_the_answer_is_ignored() {
        // An rc that prints a banner would otherwise be taken for a path.
        let (bin, path) = parse_probe("welcome!\n/usr/local/bin/claude\n__NOTEMD_PATH__/x\n");
        assert_eq!(bin, Some(PathBuf::from("/usr/local/bin/claude")));
        assert_eq!(path.as_deref(), Some("/x"));
    }

    #[test]
    fn the_login_path_comes_first_but_never_drops_what_was_inherited() {
        let home = PathBuf::from("/home/u");
        let got = runtime_path_with(Some("/opt/homebrew/bin:/usr/bin"), "/usr/bin:/sbin", &home);
        let dirs: Vec<&str> = got.split(':').collect();
        assert_eq!(dirs[0], "/opt/homebrew/bin");
        assert!(
            dirs.contains(&"/sbin"),
            "inherited entries must survive: {got}"
        );
        assert_eq!(
            dirs.iter().filter(|d| **d == "/usr/bin").count(),
            1,
            "no duplicates: {got}"
        );
    }

    #[test]
    fn without_a_login_shell_the_usual_install_dirs_are_still_reachable() {
        // The whole point: a GUI-inherited PATH has no node/npx, so every stdio
        // MCP server fails to spawn.
        let got = runtime_path_with(None, "/usr/bin:/bin", &PathBuf::from("/home/u"));
        for want in ["/opt/homebrew/bin", "/usr/local/bin", "/home/u/.local/bin"] {
            assert!(
                got.split(':').any(|d| d == want),
                "{want} missing from {got}"
            );
        }
        assert!(got.split(':').any(|d| d == "/usr/bin"));
    }
}
