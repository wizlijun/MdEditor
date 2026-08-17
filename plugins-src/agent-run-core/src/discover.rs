//! Locate a harness executable. A GUI process inherits a lean PATH that has
//! none of the user's shell additions, so a plain `Command::new("claude")` (or
//! `"dsh-acp-demo"`) fails for most installs. Three tiers: explicit path →
//! login-shell lookup → well-known install locations.
//!
//! Generalized from claude-agent: the binary NAME is a parameter, and the
//! expensive login-shell probe is cached per name.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Pure core, injectable for tests: `shell_lookup` stands in for the login
/// shell's `command -v <bin>`, `is_exec` for "exists and is executable".
pub fn discover_with(
    explicit: Option<&str>,
    candidates: &[PathBuf],
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
    candidates.iter().find(|c| is_exec(c)).cloned()
}

const PATH_MARKER: &str = "__NOTEMD_PATH__";

/// 一个 login+interactive 的 zsh 要把用户整份 rc 跑一遍 —— 几百毫秒起步,rc 重的
/// 机器上要好几秒。而探测是在插件的协议读循环里同步调用的:每起一次 run 就把读
/// 循环按住那么久,期间这个进程的应答全都发不出去。装在哪儿这件事一个进程生命
/// 周期内不会变,每个二进制名查一次就够;结果仍要过 `is_exec`(见
/// `discover_with`),所以缓存到的路径万一被卸载了,还是会退回到候选目录。
///
/// 同一次探测顺手把登录 shell 的 `PATH` 也带回来:被拉起的进程自己还要靠 `PATH`
/// 去找 node/npx(dsh 是 Node 程序,claude 要拉 stdio MCP server),而 GUI 继承
/// 的那份里什么都没有。两件事合一次问,不多付一次 rc 的钱。
pub fn probe(bin: &str) -> (Option<PathBuf>, Option<String>) {
    static CACHED: OnceLock<Mutex<HashMap<String, (Option<PathBuf>, Option<String>)>>> =
        OnceLock::new();
    let cache = CACHED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(bin) {
        return hit.clone();
    }
    let out = std::process::Command::new("/bin/zsh")
        .args([
            "-lic",
            &format!(r#"command -v {bin}; echo "{PATH_MARKER}$PATH""#),
        ])
        .output()
        .map(|o| parse_probe(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or((None, None));
    cache
        .lock()
        .unwrap()
        .insert(bin.to_string(), out.clone());
    out
}

/// Split the probe's stdout into the binary path and the login `PATH`.
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

/// The `PATH` a spawned harness should see. The login shell's comes first — it
/// is the one the user actually installs things into — then the usual install
/// dirs as a net for when there is no login shell to ask, then whatever this
/// process inherited, so nothing that used to resolve stops resolving.
pub fn runtime_path(bin: &str) -> String {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let inherited = std::env::var("PATH").unwrap_or_default();
    runtime_path_with(probe(bin).1.as_deref(), &inherited, &home)
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

pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands(home: &Path) -> Vec<PathBuf> {
        vec![
            home.join(".local/bin/tool"),
            PathBuf::from("/opt/homebrew/bin/tool"),
        ]
    }

    #[test]
    fn explicit_path_wins() {
        let got = discover_with(
            Some("/custom/tool"),
            &cands(Path::new("/home/u")),
            || Some(PathBuf::from("/shell/tool")),
            |_| true,
        );
        assert_eq!(got, Some(PathBuf::from("/custom/tool")));
    }

    #[test]
    fn falls_back_to_shell_lookup_when_explicit_missing() {
        let got = discover_with(
            Some("/gone/tool"),
            &cands(Path::new("/home/u")),
            || Some(PathBuf::from("/shell/tool")),
            |p| p != Path::new("/gone/tool"),
        );
        assert_eq!(got, Some(PathBuf::from("/shell/tool")));
    }

    #[test]
    fn falls_back_to_candidates_in_order_when_the_shell_finds_nothing() {
        let home = Path::new("/home/u");
        let got = discover_with(None, &cands(home), || None, |_| true);
        assert_eq!(got, Some(home.join(".local/bin/tool")));
    }

    #[test]
    fn skips_a_candidate_that_is_not_executable() {
        let home = Path::new("/home/u");
        let want = PathBuf::from("/opt/homebrew/bin/tool");
        let w = want.clone();
        let got = discover_with(None, &cands(home), || None, move |p| p == w);
        assert_eq!(got, Some(want));
    }

    #[test]
    fn returns_none_when_nothing_is_executable() {
        assert_eq!(
            discover_with(None, &cands(Path::new("/home/u")), || None, |_| false),
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
    fn a_path_survives_a_shell_that_could_not_find_the_binary() {
        let (bin, path) = parse_probe("__NOTEMD_PATH__/a/b\n");
        assert_eq!(bin, None);
        assert_eq!(path.as_deref(), Some("/a/b"));
    }

    #[test]
    fn rc_file_chatter_before_the_answer_is_ignored() {
        let (bin, path) = parse_probe("welcome!\n/usr/local/bin/dsh\n__NOTEMD_PATH__/x\n");
        assert_eq!(bin, Some(PathBuf::from("/usr/local/bin/dsh")));
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
        // The whole point: a GUI-inherited PATH has no node/npx, so a Node-based
        // harness (dsh) cannot start at all.
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
