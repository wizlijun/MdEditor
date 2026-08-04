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

/// 一个 login+interactive 的 zsh 要把用户整份 rc 跑一遍 —— 几百毫秒起步,rc 重的
/// 机器上要好几秒。而 `discover` 是在插件的协议读循环里同步调用的:每起一次 run
/// 就把读循环按住那么久,期间这个进程的应答全都发不出去。装在哪儿这件事一个进程
/// 生命周期内不会变,查一次就够;结果仍要过 `is_exec`(见 `discover_with`),所以
/// 缓存到的路径万一被卸载了,还是会退回到候选目录,不会指着一个不存在的 claude。
fn shell_lookup() -> Option<PathBuf> {
    static CACHED: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHED
        .get_or_init(|| {
            let out = std::process::Command::new("/bin/zsh")
                .args(["-lic", "command -v claude"])
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
        .clone()
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
}
