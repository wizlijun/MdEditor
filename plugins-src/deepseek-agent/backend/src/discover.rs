//! Find the user's DeepSeek Harness and get an ACP server out of it.
//!
//! ## What a user actually installs
//!
//! ```text
//! npm i -g @deepseek-ai/dsh                                # the harness
//! dsh plugin --profile notemd add @deepseek-ai/dsh-acp     # the ACP bridge
//! dsh --profile notemd --patch <vault>/.notemd/dsh/cordis.patch.yml
//! ```
//!
//! Worth spelling out, because an earlier version of this file got it wrong.
//! `@deepseek-ai/dsh-acp-demo` cannot be installed standalone — its peers
//! (`dsh-workspace-context`, `dsh-bash-env`) are unpublished — and from that I
//! concluded npm was a dead end and fell back to driving a source checkout
//! through pnpm. Wrong: a profile installs with `nodeLinker: hoisted` and
//! `autoInstallPeers: false` precisely so those peers **fall through to the dsh
//! installation's own `node_modules`** (`packages/boot/app-boot/src/profile.ts`).
//! They are not missing; dsh supplies them. Installing into an empty directory
//! was testing the one configuration that cannot work.
//!
//! The checkout path went with it. It needed the repository's dev toolchain — a
//! pinned pnpm, `tsx`, a 900-package install — none of which a user has.
use agent_run_core::discover as core;
use std::path::{Path, PathBuf};

/// The harness executable.
pub const BIN: &str = "dsh";

/// The profile note.md keeps its ACP bridge in.
///
/// Its own, not the user's `web` or `headless` profile: this one gets a bridge
/// mounted and HMR disabled, and neither belongs in a profile they drive
/// interactively.
pub const PROFILE: &str = "notemd";

/// The package that turns a profile into an ACP server.
pub const ACP_PACKAGE: &str = "@deepseek-ai/dsh-acp";

/// How to start the ACP server.
#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    /// The `dsh` executable.
    pub program: PathBuf,
    /// Where it was resolved from — shown in the window and the run record, so
    /// "which dsh was that?" has an answer.
    pub origin: String,
}

impl Launcher {
    /// The argv for one ACP run: boot our profile with the vault's overlay on
    /// top.
    ///
    /// `--patch` is repeatable and applies AFTER the profile's own layer, which
    /// is what lets the vault file stay authoritative without restating the 78
    /// rows `dsh-base` already provides.
    pub fn args(&self, patch: &Path) -> Vec<String> {
        vec![
            "--profile".into(),
            PROFILE.into(),
            "--patch".into(),
            patch.to_string_lossy().into_owned(),
        ]
    }
}

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin").join(BIN),
        PathBuf::from("/opt/homebrew/bin").join(BIN),
        PathBuf::from("/usr/local/bin").join(BIN),
        home.join(".npm-global/bin").join(BIN),
    ]
}

/// `$DSH_HOME`, or the default the harness itself uses.
pub fn dsh_home(home: &Path) -> PathBuf {
    std::env::var("DSH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".dsh"))
}

/// Where `dsh plugin --profile notemd …` installs to.
pub fn profile_dir(home: &Path) -> PathBuf {
    dsh_home(home).join("profiles").join(PROFILE)
}

/// Is the ACP bridge already in our profile?
///
/// Read from the profile manifest rather than by running `dsh plugin` every
/// launch: that command shells out to pnpm, and doing it per run would put a
/// package-manager round trip in front of every agent request.
pub fn acp_installed(home: &Path) -> bool {
    let p = profile_dir(home).join("package.json");
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("dependencies")?.get(ACP_PACKAGE).cloned())
        .is_some()
}

/// Pure core, injectable for tests. `explicit` is the plugin setting, then
/// `$NOTEMD_DSH_BIN`.
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn(&str) -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<Launcher> {
    let p = core::discover_with(explicit, &candidates(home), || shell_lookup(BIN), &is_exec)?;
    let origin = match explicit {
        Some(e) if !e.is_empty() && Path::new(e) == p => "explicit setting".to_string(),
        _ => p.to_string_lossy().into_owned(),
    };
    Some(Launcher { program: p, origin })
}

/// Production entry.
pub fn discover(explicit: Option<&str>) -> Option<Launcher> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let explicit = explicit
        .map(str::to_string)
        .or_else(|| std::env::var("NOTEMD_DSH_BIN").ok());
    discover_with(
        explicit.as_deref(),
        &home,
        |bin| core::probe(bin).0,
        core::is_executable,
    )
}

/// Put the ACP bridge into our profile. Idempotent; a no-op once installed.
///
/// Everything goes through the upstream CLI — version resolution, the lockfile,
/// the bundle reconcile are its semantics, not ours. This plugin never writes
/// inside `$DSH_HOME` itself.
pub fn ensure_acp(dsh: &Path, home: &Path, path_env: &str) -> Result<bool, String> {
    if acp_installed(home) {
        return Ok(false);
    }
    let out = std::process::Command::new(dsh)
        .args(["plugin", "--profile", PROFILE, "add", ACP_PACKAGE])
        .env("PATH", path_env)
        .output()
        .map_err(|e| format!("could not run `{} plugin`: {e}", dsh.display()))?;
    if !out.status.success() {
        let why = agent_run_core::harness::first_line(&String::from_utf8_lossy(&out.stderr))
            .or_else(|| agent_run_core::harness::first_line(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_else(|| "no output".into());
        return Err(format!(
            "把 {ACP_PACKAGE} 装进 `{PROFILE}` profile 失败:{why}\n\
             在终端自己跑一次能看到完整输出:\n  \
             dsh plugin --profile {PROFILE} add {ACP_PACKAGE}"
        ));
    }
    Ok(true)
}

/// The `PATH` the harness should see. dsh is a Node program that spawns its own
/// tooling (pnpm, npx); a GUI-launched host inherits a `PATH` with no node.
pub fn runtime_path() -> String {
    core::runtime_path(BIN)
}

/// What to tell the user when `dsh` is nowhere to be found.
pub const NOT_FOUND: &str = "没找到 DeepSeek Harness(`dsh`)。\n\
     装它:`npm i -g @deepseek-ai/dsh`。\n\
     ACP 桥由插件自动装进它的 `notemd` profile,你不用手动配。\n\
     已经装在别处的话,用插件设置 `dsh_bin` 或环境变量 NOTEMD_DSH_BIN 指过去。";

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: fn(&str) -> Option<PathBuf> = |_| None;

    /// `DSH_HOME` is process-global; the tests that set it take turns.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn an_explicit_path_wins() {
        let got = discover_with(
            Some("/custom/dsh"),
            Path::new("/home/u"),
            |_| Some(PathBuf::from("/shell/dsh")),
            |_| true,
        )
        .unwrap();
        assert_eq!(got.program, PathBuf::from("/custom/dsh"));
        assert_eq!(got.origin, "explicit setting");
    }

    #[test]
    fn falls_back_to_the_login_shell_then_to_the_usual_places() {
        let got = discover_with(
            Some("/gone/dsh"),
            Path::new("/home/u"),
            |b| (b == BIN).then(|| PathBuf::from("/opt/homebrew/bin/dsh")),
            |p| p != Path::new("/gone/dsh"),
        )
        .unwrap();
        assert_eq!(got.program, PathBuf::from("/opt/homebrew/bin/dsh"));

        let home = Path::new("/home/u");
        let want = home.join(".local/bin/dsh");
        let w = want.clone();
        let got = discover_with(None, home, NONE, move |p| p == w).unwrap();
        assert_eq!(got.program, want);
    }

    #[test]
    fn returns_none_when_dsh_is_not_installed() {
        assert_eq!(
            discover_with(None, Path::new("/home/u"), NONE, |_| false),
            None
        );
    }

    /// One run = one boot of our own profile with the vault's overlay on top.
    #[test]
    fn the_argv_boots_our_profile_with_the_vault_overlay() {
        let l = Launcher {
            program: PathBuf::from("/opt/homebrew/bin/dsh"),
            origin: "x".into(),
        };
        assert_eq!(
            l.args(Path::new("/v/.notemd/dsh/cordis.patch.yml")),
            vec![
                "--profile",
                "notemd",
                "--patch",
                "/v/.notemd/dsh/cordis.patch.yml"
            ]
        );
    }

    /// Checked from the manifest, not by running the installer: `dsh plugin`
    /// shells out to pnpm, and doing that per run would put a package-manager
    /// round trip in front of every agent request.
    #[test]
    fn the_bridge_is_detected_from_the_profile_manifest() {
        let _g = env_guard();
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("DSH_HOME", d.path());
        let home = Path::new("/unused");
        assert!(!acp_installed(home), "an empty home has no bridge");

        let prof = d.path().join("profiles/notemd");
        std::fs::create_dir_all(&prof).unwrap();
        std::fs::write(prof.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        assert!(!acp_installed(home), "a profile without the package is not ready");

        std::fs::write(
            prof.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-acp":"0.0.1-rc.1"}}"#,
        )
        .unwrap();
        assert!(acp_installed(home));

        std::fs::write(prof.join("package.json"), "{not json").unwrap();
        assert!(!acp_installed(home), "an unreadable manifest is not proof");
        std::env::remove_var("DSH_HOME");
    }

    /// Already installed ⇒ no package-manager round trip at all.
    #[test]
    fn ensuring_the_bridge_is_a_no_op_once_it_is_there() {
        let _g = env_guard();
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("DSH_HOME", d.path());
        let prof = d.path().join("profiles/notemd");
        std::fs::create_dir_all(&prof).unwrap();
        std::fs::write(
            prof.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-acp":"0.0.1-rc.1"}}"#,
        )
        .unwrap();
        // A path that would fail loudly if it were ever executed.
        assert_eq!(
            ensure_acp(Path::new("/nonexistent/dsh"), Path::new("/unused"), "/usr/bin"),
            Ok(false)
        );
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn a_failed_install_reports_the_command_to_run_by_hand() {
        let _g = env_guard();
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("DSH_HOME", d.path());
        let e = ensure_acp(Path::new("/nonexistent/dsh"), Path::new("/unused"), "/usr/bin")
            .unwrap_err();
        assert!(e.contains("could not run"), "{e}");
        std::env::remove_var("DSH_HOME");
    }

    /// The hint has to name the thing that actually works: `dsh-acp-demo` from
    /// npm does not install, and a user has no source checkout.
    #[test]
    fn the_not_found_message_names_the_real_install() {
        assert!(NOT_FOUND.contains("npm i -g @deepseek-ai/dsh"));
        assert!(NOT_FOUND.contains("NOTEMD_DSH_BIN"));
        assert!(
            !NOT_FOUND.contains("acp-demo"),
            "that package cannot be installed standalone"
        );
        assert!(
            !NOT_FOUND.contains("checkout"),
            "users do not have a source checkout"
        );
    }

    #[test]
    fn every_candidate_is_an_absolute_dsh_path() {
        for c in candidates(Path::new("/home/u")) {
            assert!(c.is_absolute(), "{c:?}");
            assert_eq!(c.file_name().unwrap(), BIN);
        }
    }
}
