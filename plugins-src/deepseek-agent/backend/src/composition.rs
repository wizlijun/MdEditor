//! The harness OVERLAY that lives in the vault.
//!
//! The composition itself is the user's dsh profile — `dsh plugin --profile
//! notemd add @deepseek-ai/dsh-acp` puts the bridge there, and `dsh-base`
//! supplies the other 78 rows. This file is the `--patch` layer applied on top:
//! it mounts the ACP row, points the sandbox at the mode the task asked for, and
//! silences HMR (which would write to the stdout the protocol owns).
//!
//! It lives in the vault rather than in `$DSH_HOME` because it is a statement
//! about how NOTE.MD uses the harness, not about the harness itself: plain text,
//! diffable, synced with the vault, editable by hand (belief 2, files over app).
//! `$DSH_HOME` stays entirely upstream's — this plugin never writes there, it
//! only ever asks `dsh plugin` to.
use std::path::{Path, PathBuf};

/// The composition compiled into this binary.
const DEFAULT_CONFIG: &str = include_str!("../templates/_dsh/cordis.patch.yml");

/// Where the vault keeps harness state that is ours to manage.
pub fn dsh_dir(vault: &Path) -> PathBuf {
    vault.join(".notemd/dsh")
}

pub fn config_path(vault: &Path) -> PathBuf {
    dsh_dir(vault).join("cordis.patch.yml")
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
    retire_legacy_composition(vault);
    let p = config_path(vault);
    if std::fs::read_to_string(&p).is_ok_and(|cur| cur == DEFAULT_CONFIG) {
        return Ok(false);
    }
    std::fs::create_dir_all(dsh_dir(vault))?;
    std::fs::write(&p, DEFAULT_CONFIG)?;
    Ok(true)
}

/// Remove the full composition an earlier build wrote (`cordis.yml`).
///
/// That file drove a standalone `dsh-acp-demo`, which is not how this works any
/// more: the composition is the user's dsh profile and we ship an overlay. It is
/// inert now, but a `cordis.yml` sitting beside `cordis.patch.yml` invites the
/// question of which one is live.
///
/// Only removed while it is still recognisably OURS — the header we wrote. A
/// user who edited it has said something we should not throw away.
fn retire_legacy_composition(vault: &Path) {
    let legacy = dsh_dir(vault).join("cordis.yml");
    let ours = std::fs::read_to_string(&legacy)
        .is_ok_and(|s| s.starts_with("# DeepSeek Harness 组合配置 —— note.md 专用"));
    if ours {
        let _ = std::fs::remove_file(&legacy);
    }
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
        // Rows appear both at the top level (`- id: hmr`) and nested inside an
        // `- insert:` list (`- id: acp`), so match on the id itself rather than
        // on nesting. Any row whose id is exactly `acp` is the bridge.
        if let Some(id) = t.strip_prefix("- id:") {
            in_acp = id.trim() == "acp";
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

    /// The previous design's file is superseded, and two config files in one
    /// directory is a question nobody should have to answer.
    #[test]
    fn the_previous_designs_composition_is_retired() {
        let v = tempfile::tempdir().unwrap();
        let legacy = dsh_dir(v.path()).join("cordis.yml");
        std::fs::create_dir_all(dsh_dir(v.path())).unwrap();
        std::fs::write(
            &legacy,
            "# DeepSeek Harness 组合配置 —— note.md 专用(notemd.deepseek-agent 下发)\n- id: x\n",
        )
        .unwrap();
        ensure_config(v.path()).unwrap();
        assert!(!legacy.exists(), "our own superseded file should go");
        assert!(config_path(v.path()).is_file());
    }

    /// A file the user made their own is not ours to delete.
    #[test]
    fn a_hand_written_cordis_yml_is_left_alone() {
        let v = tempfile::tempdir().unwrap();
        let mine = dsh_dir(v.path()).join("cordis.yml");
        std::fs::create_dir_all(dsh_dir(v.path())).unwrap();
        std::fs::write(&mine, "# my own composition\n- id: x\n").unwrap();
        ensure_config(v.path()).unwrap();
        assert!(mine.exists(), "a user-written file must survive");
    }

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
        // HMR must be present — but DISABLED. `dsh-base` mounts it, so the
        // overlay silencing it is exactly what keeps stdout clean.
        assert!(
            DEFAULT_CONFIG.contains("- id: hmr\n  disabled: true"),
            "the overlay must disable HMR, which dsh-base mounts and which writes to stdout"
        );
        for forbidden in ["dsh-logger-stdout", "console.log"] {
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
        assert!(DEFAULT_CONFIG.contains("sandbox-policy"));
        assert!(DEFAULT_CONFIG.contains("approval"));
        assert_eq!(
            DEFAULT_CONFIG.matches("DSH_PERMISSION_MODE").count(),
            2,
            "both the sandbox fence and the approval gate must read the mode"
        );
        assert!(DEFAULT_CONFIG.contains("@deepseek-ai/dsh-acp"));
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
            "- id: llm-deepseek\n  config:\n    model: not-this-one\n\n- insert:\n    - id: acp\n      name: '@deepseek-ai/dsh-acp'\n      config:\n        provider: deepseek-official\n        model: deepseek-v4-flash\n",
        )
        .unwrap();
        assert_eq!(default_model(&p).as_deref(), Some("deepseek-v4-flash"));
    }

    #[test]
    fn a_composition_without_a_model_reads_as_unknown() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bare.yml");
        std::fs::write(&p, "- insert:\n    - id: acp\n      name: '@deepseek-ai/dsh-acp'\n").unwrap();
        assert_eq!(default_model(&p), None);
        assert_eq!(default_model(&d.path().join("gone.yml")), None);
    }

    /// A key inlined here would be committed to the user's vault.
    #[test]
    fn the_shipped_composition_carries_no_credentials() {
        assert!(
            !DEFAULT_CONFIG.contains("sk-"),
            "no API key may be inlined in a file that goes into git"
        );
    }
}
