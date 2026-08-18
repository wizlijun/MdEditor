//! Find something that can serve ACP over stdio.
//!
//! `@deepseek-ai/dsh-acp` is a transport adapter, not a runnable program: it
//! needs `ctx.agents`, an LLM adapter, a sandbox and a tool stack around it. The
//! runnable composition is `@deepseek-ai/dsh-acp-demo`, which ships a
//! `dsh-acp-demo` bin that boots an ACP stdio server from a `cordis.yml`.
//!
//! Five tiers, in order. The last one exists because the npm packages lag the
//! monorepo (`dsh` is at 0.1.0-rc.6 while `dsh-acp*` sits at 0.0.1-rc.1), so a
//! contributor with a checkout should be able to run against it directly.
use agent_run_core::discover as core;
use std::path::{Path, PathBuf};

/// The published bin name.
pub const BIN: &str = "dsh-acp-demo";

/// How to start the ACP server. Two shapes, because a monorepo checkout is run
/// through its package manager rather than as a bare executable.
#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The version, when it can be read WITHOUT running anything (a checkout's
    /// package.json). `None` means "ask the executable".
    pub known_version: Option<String>,
    /// Where the program is resolved from — shown in the run record so a support
    /// question ("which dsh was that?") has an answer.
    pub origin: String,
}

impl Launcher {
    fn bin(path: PathBuf, origin: &str) -> Self {
        Self {
            program: path,
            args: Vec::new(),
            known_version: None,
            origin: origin.to_string(),
        }
    }
}

/// Well-known install locations for the published bin, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin").join(BIN),
        PathBuf::from("/opt/homebrew/bin").join(BIN),
        PathBuf::from("/usr/local/bin").join(BIN),
        // npm's global prefix on the two layouts Homebrew produces.
        PathBuf::from("/opt/homebrew/lib/node_modules/@deepseek-ai/dsh-acp-demo/lib/bin.js"),
        home.join(".npm-global/bin").join(BIN),
    ]
}

/// The version a checkout reports, read from the ACP app's own package.json.
///
/// Asking the launcher instead would mean running `pnpm run demo:acp --version`,
/// which does not handle `--version`: it boots an ACP server that then sits
/// waiting on stdin until the probe times out. Twenty seconds to learn nothing.
/// The file says it instantly and says it accurately.
pub fn repo_version(repo: &Path) -> Option<String> {
    for rel in [
        "packages/examples/acp-demo/package.json",
        "package.json",
    ] {
        let body = std::fs::read_to_string(repo.join(rel)).ok();
        let v = body
            .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("version")?.as_str().map(str::to_string));
        if let Some(v) = v {
            return Some(v);
        }
    }
    None
}

/// Monorepo checkouts worth looking in, in priority order.
pub fn repo_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("git/deepseek-harness"),
        home.join("src/deepseek-harness"),
        home.join("deepseek-harness"),
    ]
}

/// Is this directory a DeepSeek Harness checkout that can boot an ACP server?
///
/// Checked by CONTENT, not by name: the marker is the `demo:acp` script in the
/// root package.json, which is the thing we would actually invoke. A directory
/// that merely has the right name is not enough.
pub fn is_harness_repo(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/scripts/demo:acp").cloned())
        .is_some()
}

/// Run an ACP server straight out of a monorepo checkout. `pnpm --dir <repo> run
/// demo:acp` is the repository's own documented entry point ("Running", in
/// `packages/acp/acp/README.md`), so this stays on a supported path rather than
/// reaching into the package layout.
fn repo_launcher(pnpm: PathBuf, repo: &Path) -> Launcher {
    Launcher {
        program: pnpm,
        args: vec![
            // The harness repo pins `packageManager` (pnpm@11.7.0). When the
            // user's pnpm differs, corepack refuses to run AT ALL rather than
            // switching — "This project is configured to use 11.7.0 of pnpm.
            // Your current pnpm is v11.0.9". That pin protects the repo's own
            // contributors from lockfile churn; it is not a statement about
            // whether the ACP server can boot, which is all we want from it.
            // Downgrading it to a warning is the difference between "DeepSeek
            // works here" and "DeepSeek is unavailable" on any machine whose
            // global pnpm is not that exact version.
            "--pm-on-fail=ignore".into(),
            // pnpm 11 在 `run` 前会校验依赖并**静默补装**:进度全在 stdout、
            // 一装数分钟、网络一抖就 exit 1,ACP 服务根本没起来(2026-08-18 事故)。
            // 关掉它:依赖残缺就让脚本在 2 秒内带着清晰的 stderr 快死 ——
            // 「先去 checkout 里跑一次 pnpm install」是用户能看懂的下一步。
            "--config.verify-deps-before-run=false".into(),
            "--dir".into(),
            repo.to_string_lossy().into_owned(),
            "run".into(),
            "demo:acp".into(),
        ],
        known_version: repo_version(repo),
        origin: format!("monorepo checkout at {}", repo.display()),
    }
}

/// Pure core, injectable for tests.
///
/// * `explicit` — the plugin setting, then `$NOTEMD_DSH_ACP_BIN`.
/// * `explicit_repo` — the plugin setting, then `$DSH_REPO`.
#[allow(clippy::too_many_arguments)]
pub fn discover_with(
    explicit: Option<&str>,
    explicit_repo: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn(&str) -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
    is_repo: impl Fn(&Path) -> bool,
) -> Option<Launcher> {
    // 1-4: a real executable, by the shared three-tier mechanism.
    if let Some(p) = core::discover_with(
        explicit,
        &candidates(home),
        || shell_lookup(BIN),
        &is_exec,
    ) {
        let origin = match explicit {
            Some(e) if !e.is_empty() && Path::new(e) == p => "explicit setting".to_string(),
            _ => format!("{} on PATH", BIN),
        };
        return Some(Launcher::bin(p, &origin));
    }

    // 5: a checkout, driven through pnpm. Useless without pnpm, so resolve that
    // first and give up quietly rather than emitting a launcher that cannot run.
    let pnpm = shell_lookup("pnpm")
        .filter(|p| is_exec(p))
        .or_else(|| {
            [
                PathBuf::from("/opt/homebrew/bin/pnpm"),
                PathBuf::from("/usr/local/bin/pnpm"),
                home.join(".local/bin/pnpm"),
            ]
            .into_iter()
            .find(|p| is_exec(p))
        })?;
    let repo = explicit_repo
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| is_repo(p))
        .or_else(|| repo_candidates(home).into_iter().find(|p| is_repo(p)))?;
    Some(repo_launcher(pnpm, &repo))
}

/// Production entry.
pub fn discover(explicit: Option<&str>, explicit_repo: Option<&str>) -> Option<Launcher> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let explicit = explicit
        .map(str::to_string)
        .or_else(|| std::env::var("NOTEMD_DSH_ACP_BIN").ok());
    let explicit_repo = explicit_repo
        .map(str::to_string)
        .or_else(|| std::env::var("DSH_REPO").ok());
    discover_with(
        explicit.as_deref(),
        explicit_repo.as_deref(),
        &home,
        |bin| core::probe(bin).0,
        core::is_executable,
        is_harness_repo,
    )
}

/// The `PATH` the ACP server should see. dsh is a Node program that also spawns
/// its own tooling, and a GUI-launched host inherits a `PATH` with no node.
pub fn runtime_path() -> String {
    core::runtime_path(BIN)
}

/// What to tell the user when nothing was found.
///
/// It deliberately does NOT recommend `npm i -g @deepseek-ai/dsh-acp-demo`,
/// which is the obvious advice and does not work: that package (0.0.1-rc.1)
/// declares required peers — `@deepseek-ai/dsh-workspace-context`,
/// `@deepseek-ai/dsh-bash-env` — that were never published, so the install
/// either refuses outright or produces a binary that cannot start. Verified
/// 2026-08-18. Sending someone down a path we know is broken is worse than
/// admitting the harness is preview-quality.
pub const NOT_FOUND: &str = "DeepSeek Harness 的 ACP 服务端没找到。\n\
     目前唯一可用的装法是本地 checkout:克隆 deepseek-harness 仓库,在里面跑一次\n\
     `pnpm install`,插件会自动发现它(也可用插件设置 `dsh_repo` 或环境变量 DSH_REPO 指定)。\n\
     npm 上的 @deepseek-ai/dsh-acp-demo 暂时装不起来 —— 它依赖的\n\
     dsh-workspace-context / dsh-bash-env 还没发布到 npm(2026-08-18 实测)。\n\
     已有可执行文件的话,用 `dsh_acp_bin` 或 NOTEMD_DSH_ACP_BIN 直接指过去。";

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: fn(&str) -> Option<PathBuf> = |_| None;

    #[test]
    fn an_explicit_executable_wins_over_everything() {
        let got = discover_with(
            Some("/custom/acp"),
            Some("/home/u/git/deepseek-harness"),
            Path::new("/home/u"),
            |_| Some(PathBuf::from("/shell/dsh-acp-demo")),
            |_| true,
            |_| true,
        )
        .unwrap();
        assert_eq!(got.program, PathBuf::from("/custom/acp"));
        assert!(got.args.is_empty());
        assert_eq!(got.origin, "explicit setting");
    }

    #[test]
    fn falls_back_to_the_binary_on_path() {
        let got = discover_with(
            Some("/gone/acp"),
            None,
            Path::new("/home/u"),
            |b| (b == BIN).then(|| PathBuf::from("/shell/dsh-acp-demo")),
            |p| p != Path::new("/gone/acp"),
            |_| false,
        )
        .unwrap();
        assert_eq!(got.program, PathBuf::from("/shell/dsh-acp-demo"));
        assert!(got.origin.contains(BIN), "{}", got.origin);
    }

    #[test]
    fn falls_back_to_the_well_known_install_locations() {
        let home = Path::new("/home/u");
        let want = home.join(".local/bin").join(BIN);
        let w = want.clone();
        let got = discover_with(None, None, home, NONE, move |p| p == w, |_| false).unwrap();
        assert_eq!(got.program, want);
    }

    /// The npm packages lag the monorepo, so a contributor's checkout has to be
    /// reachable — but only through the repository's own documented entry point.
    #[test]
    fn falls_back_to_a_monorepo_checkout_through_pnpm() {
        let home = Path::new("/home/u");
        let repo = home.join("git/deepseek-harness");
        let r = repo.clone();
        let got = discover_with(
            None,
            None,
            home,
            |b| (b == "pnpm").then(|| PathBuf::from("/opt/homebrew/bin/pnpm")),
            |p| p == Path::new("/opt/homebrew/bin/pnpm"),
            move |p| p == r,
        )
        .unwrap();
        assert_eq!(got.program, PathBuf::from("/opt/homebrew/bin/pnpm"));
        assert_eq!(
            got.args,
            vec![
                "--pm-on-fail=ignore",
                "--config.verify-deps-before-run=false",
                "--dir",
                &repo.to_string_lossy(),
                "run",
                "demo:acp"
            ]
        );
        assert!(got.origin.contains("monorepo"), "{}", got.origin);
    }

    #[test]
    fn an_explicit_repo_setting_beats_the_well_known_checkouts() {
        let home = Path::new("/home/u");
        let got = discover_with(
            None,
            Some("/work/dsh"),
            home,
            |b| (b == "pnpm").then(|| PathBuf::from("/bin/pnpm")),
            |p| p == Path::new("/bin/pnpm"),
            |_| true,
        )
        .unwrap();
        assert_eq!(got.args[3], "/work/dsh");
    }

    /// A launcher that names pnpm we could not find would fail at spawn with a
    /// confusing error; better to report "not found" and print the install hint.
    #[test]
    fn a_checkout_without_pnpm_is_not_a_launcher() {
        let home = Path::new("/home/u");
        assert_eq!(
            discover_with(None, None, home, NONE, |_| false, |_| true),
            None
        );
    }

    #[test]
    fn returns_none_when_there_is_nothing_at_all() {
        assert_eq!(
            discover_with(None, None, Path::new("/home/u"), NONE, |_| false, |_| false),
            None
        );
    }

    /// Recognized by the script we would actually invoke, not by directory name:
    /// a same-named directory that cannot boot an ACP server is not a checkout.
    #[test]
    fn a_harness_checkout_is_recognized_by_its_demo_acp_script() {
        let d = tempfile::tempdir().unwrap();
        assert!(!is_harness_repo(d.path()), "an empty dir is not a checkout");

        std::fs::write(d.path().join("package.json"), r#"{"name":"something-else"}"#).unwrap();
        assert!(!is_harness_repo(d.path()), "a name alone is not enough");

        std::fs::write(
            d.path().join("package.json"),
            r#"{"name":"deepseek-harness","scripts":{"demo:acp":"node …"}}"#,
        )
        .unwrap();
        assert!(is_harness_repo(d.path()));

        std::fs::write(d.path().join("package.json"), "{not json").unwrap();
        assert!(!is_harness_repo(d.path()), "unparseable is not a checkout");
    }

    /// Asking the launcher would boot an ACP server and time out; the file
    /// answers instantly and accurately.
    #[test]
    fn a_checkout_reports_its_version_from_disk_without_running_anything() {
        let d = tempfile::tempdir().unwrap();
        let acp = d.path().join("packages/examples/acp-demo");
        std::fs::create_dir_all(&acp).unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"version":"0.1.0-rc.5"}"#).unwrap();
        std::fs::write(acp.join("package.json"), r#"{"version":"0.1.0-rc.5"}"#).unwrap();
        assert_eq!(repo_version(d.path()).as_deref(), Some("0.1.0-rc.5"));
    }

    #[test]
    fn a_checkout_falls_back_to_the_root_version_then_to_nothing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("package.json"), r#"{"version":"9.9.9"}"#).unwrap();
        assert_eq!(repo_version(d.path()).as_deref(), Some("9.9.9"));

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(repo_version(empty.path()), None);
    }

    #[test]
    fn every_candidate_is_absolute() {
        let home = Path::new("/home/u");
        for c in candidates(home).into_iter().chain(repo_candidates(home)) {
            assert!(c.is_absolute(), "{c:?}");
        }
    }

    /// The hint has to point somewhere that WORKS. `npm i -g dsh-acp-demo` is
    /// the obvious advice and is a dead end — its peers are unpublished — so the
    /// message must not present it as the fix.
    #[test]
    fn the_not_found_message_names_a_way_out_that_actually_works() {
        assert!(NOT_FOUND.contains("checkout"));
        assert!(NOT_FOUND.contains("pnpm install"));
        assert!(NOT_FOUND.contains("dsh_acp_bin"));
        assert!(NOT_FOUND.contains("DSH_REPO"));
        assert!(
            NOT_FOUND.contains("装不起来"),
            "the npm route must be named as broken, not offered as the fix"
        );
    }

    /// Without this the harness is unavailable on any machine whose global pnpm
    /// is not the exact version the repo pins — corepack refuses rather than
    /// switching.
    #[test]
    fn the_checkout_launcher_downgrades_the_package_manager_pin_to_a_warning() {
        let home = Path::new("/home/u");
        let got = discover_with(
            None,
            None,
            home,
            |b| (b == "pnpm").then(|| PathBuf::from("/bin/pnpm")),
            |p| p == Path::new("/bin/pnpm"),
            |_| true,
        )
        .unwrap();
        assert_eq!(got.args[0], "--pm-on-fail=ignore");
    }
}
