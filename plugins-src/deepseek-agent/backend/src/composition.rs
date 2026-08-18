//! The harness composition that lives in the vault.
//!
//! `dsh-acp-demo` boots an ACP server from a `cordis.yml`. That file has to come
//! from somewhere, and there were two candidates:
//!
//! * `$DSH_HOME/profiles/notemd/` — dsh's own territory, managed by the upstream
//!   `dsh plugin` CLI with its own lockfile and bundle-reconcile semantics. A
//!   plugin writing files in there is fighting upstream for the steering wheel.
//!   (It also would not work: `@deepseek-ai/dsh-acp` declares no `dsh.bundle`,
//!   so `dsh plugin add` installs it as a plain dependency that never joins the
//!   profile's layer stack — see `apps/cli/src/plugin.ts`'s `reconcilePlugins`.)
//! * `<vault>/.notemd/dsh/cordis.yml` — plain text, diffable, synced with the
//!   vault, editable by hand.
//!
//! The second one. Belief 2: files over app. The plugin owns the DEFAULT
//! contents the same way it owns task templates — refreshed when stale, never
//! touched when already current — and a user who wants permanent changes points
//! `dsh_config` at their own copy, after which this code never writes again.
use std::path::{Path, PathBuf};

/// The composition compiled into this binary.
const DEFAULT_CONFIG: &str = include_str!("../templates/_dsh/cordis.yml");

/// Where the vault keeps harness state that is ours to manage.
pub fn dsh_dir(vault: &Path) -> PathBuf {
    vault.join(".notemd/dsh")
}

pub fn config_path(vault: &Path) -> PathBuf {
    dsh_dir(vault).join("cordis.yml")
}

/// Where the harness parks its session logs (its Trajectory). Derived data: the
/// run record keeps only a session id pointing into it.
pub fn sessions_dir(vault: &Path) -> PathBuf {
    dsh_dir(vault).join("sessions")
}

/// Seed or refresh the vault's composition. Returns `true` when a byte was
/// written.
///
/// Idempotent by content, like the task templates: identical content is left
/// alone so the vault's git auto-sync is not spammed with mtime churn.
pub fn ensure_config(vault: &Path) -> std::io::Result<bool> {
    let p = config_path(vault);
    if std::fs::read_to_string(&p).is_ok_and(|cur| cur == DEFAULT_CONFIG) {
        return Ok(false);
    }
    std::fs::create_dir_all(dsh_dir(vault))?;
    std::fs::write(&p, DEFAULT_CONFIG)?;
    Ok(true)
}

/// Which config this run should use: the user's override if they set one, else
/// the vault's managed copy.
///
/// An override that is not there is an ERROR rather than a silent fallback — a
/// user who pointed at a file expects THAT file's permissions and model, and
/// quietly running the default composition instead is the wrong kind of helpful.
pub fn resolve_config(vault: &Path, override_path: Option<&str>) -> Result<PathBuf, String> {
    match override_path.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            p.is_file()
                .then_some(p.clone())
                .ok_or_else(|| format!("dsh_config points at a file that is not there: {}", p.display()))
        }
        None => {
            ensure_config(vault).map_err(|e| format!("could not write the dsh composition: {e}"))?;
            Ok(config_path(vault))
        }
    }
}

/// The model a run uses when its task pins none — read out of the composition
/// the run will actually boot from, not from a constant here. A user who edited
/// their `cordis.yml` to a different model must see THAT model reported, or the
/// window is telling them something the run will contradict.
///
/// Deliberately a line scan rather than a YAML parse: the file carries `!!js`
/// tags that a plain YAML reader rejects, and one field does not justify a
/// dependency that would then need to understand them.
pub fn default_model(config: &Path) -> Option<String> {
    let body = std::fs::read_to_string(config).ok()?;
    let mut in_acp = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("- id:") {
            in_acp = t.contains("acp-agent");
            continue;
        }
        if in_acp {
            if let Some(v) = t.strip_prefix("model:") {
                let v = v.trim().trim_matches(&['\'', '"'][..]).trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Lines to keep out of the vault's git history. The session logs are the
/// harness's own append-only trace — large, churny, and reconstructible.
pub const GITIGNORE_LINES: [&str; 1] = [".notemd/dsh/sessions/"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_the_composition_on_a_fresh_vault() {
        let v = tempfile::tempdir().unwrap();
        assert!(ensure_config(v.path()).unwrap());
        let body = std::fs::read_to_string(config_path(v.path())).unwrap();
        assert_eq!(body, DEFAULT_CONFIG);
    }

    /// Rewriting an identical file every launch would show up as vault churn in
    /// the user's git history.
    #[test]
    fn leaves_an_up_to_date_composition_untouched() {
        let v = tempfile::tempdir().unwrap();
        assert!(ensure_config(v.path()).unwrap());
        assert!(!ensure_config(v.path()).unwrap());
    }

    #[test]
    fn refreshes_a_stale_composition() {
        let v = tempfile::tempdir().unwrap();
        ensure_config(v.path()).unwrap();
        std::fs::write(config_path(v.path()), "# hand-mangled\n").unwrap();
        assert!(ensure_config(v.path()).unwrap());
        assert_eq!(
            std::fs::read_to_string(config_path(v.path())).unwrap(),
            DEFAULT_CONFIG
        );
    }

    #[test]
    fn resolve_falls_back_to_the_managed_copy_and_seeds_it() {
        let v = tempfile::tempdir().unwrap();
        let got = resolve_config(v.path(), None).unwrap();
        assert_eq!(got, config_path(v.path()));
        assert!(got.is_file());
        // Blank and whitespace-only overrides mean "unset", not "a file named ' '".
        assert_eq!(resolve_config(v.path(), Some("")).unwrap(), got);
        assert_eq!(resolve_config(v.path(), Some("   ")).unwrap(), got);
    }

    #[test]
    fn an_override_is_used_verbatim_and_stops_the_plugin_writing() {
        let v = tempfile::tempdir().unwrap();
        let mine = v.path().join("my-cordis.yml");
        std::fs::write(&mine, "# mine\n").unwrap();
        let got = resolve_config(v.path(), Some(mine.to_str().unwrap())).unwrap();
        assert_eq!(got, mine);
        assert!(
            !config_path(v.path()).exists(),
            "an override must not cause the managed copy to be written"
        );
    }

    /// Silently running the default composition would give the run different
    /// permissions and a different model than the user asked for.
    #[test]
    fn a_missing_override_is_an_error_rather_than_a_silent_fallback() {
        let v = tempfile::tempdir().unwrap();
        let e = resolve_config(v.path(), Some("/nope/cordis.yml")).unwrap_err();
        assert!(e.contains("not there"), "{e}");
        assert!(!config_path(v.path()).exists());
    }

    /// stdout carries the ACP JSON-RPC frames. Anything in this tree that logs
    /// to stdout corrupts the protocol stream, so the shipped composition must
    /// not grow one.
    #[test]
    fn the_shipped_composition_mounts_nothing_that_writes_to_stdout() {
        for forbidden in ["dsh-logger-stdout", "cordis-plugin-hmr", "console"] {
            assert!(
                !DEFAULT_CONFIG.contains(forbidden),
                "{forbidden} would corrupt the ACP stream on stdout"
            );
        }
    }

    /// The permission mode is the run's real boundary; it has to reach the two
    /// rows that enforce it.
    #[test]
    fn the_shipped_composition_reads_the_permission_mode_from_the_environment() {
        assert!(DEFAULT_CONFIG.contains("dsh-sandbox-policy"));
        assert!(DEFAULT_CONFIG.contains("dsh-user-approval"));
        assert_eq!(
            DEFAULT_CONFIG.matches("DSH_PERMISSION_MODE").count(),
            2,
            "both the sandbox fence and the approval gate must read the mode"
        );
        assert!(DEFAULT_CONFIG.contains("@deepseek-ai/dsh-acp-demo"));
    }

    #[test]
    fn reads_the_acp_agents_model_out_of_the_shipped_composition() {
        let v = tempfile::tempdir().unwrap();
        ensure_config(v.path()).unwrap();
        assert_eq!(
            default_model(&config_path(v.path())).as_deref(),
            Some("deepseek-v4-pro")
        );
    }

    /// A user who edits their composition must see the model THEY chose — the
    /// window telling them one thing while the run does another is worse than
    /// telling them nothing.
    #[test]
    fn reads_an_edited_model_rather_than_a_constant() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("mine.yml");
        std::fs::write(
            &p,
            "- id: llm-deepseek\n  config:\n    model: not-this-one\n\n- id: acp-agent\n  name: '@deepseek-ai/dsh-acp-demo'\n  config:\n    provider: deepseek-official\n    model: deepseek-v4-flash\n",
        )
        .unwrap();
        assert_eq!(default_model(&p).as_deref(), Some("deepseek-v4-flash"));
    }

    #[test]
    fn a_composition_without_a_model_reads_as_unknown() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bare.yml");
        std::fs::write(&p, "- id: acp-agent\n  name: '@deepseek-ai/dsh-acp-demo'\n").unwrap();
        assert_eq!(default_model(&p), None);
        assert_eq!(default_model(&d.path().join("gone.yml")), None);
    }

    /// A key inlined here would be committed to the user's vault.
    #[test]
    fn the_shipped_composition_carries_no_credentials() {
        assert!(DEFAULT_CONFIG.contains("dsh-credentials-local"));
        assert!(
            !DEFAULT_CONFIG.contains("sk-"),
            "no API key may be inlined in a file that goes into git"
        );
    }
}
