//! A template's .claude/settings.json stays portable by writing `${VAULT}`
//! instead of a machine path. Before each run we substitute the real vault root
//! into .claude/settings.local.json — Claude Code's own local override layer,
//! already gitignored on the vault side.
use std::path::Path;

/// Generate settings.local.json. A task without a settings template is fine —
/// silently skipped.
pub fn materialize(task_dir: &Path, vault: &Path) -> std::io::Result<()> {
    let src = task_dir.join(".claude/settings.json");
    let Ok(body) = std::fs::read_to_string(&src) else {
        return Ok(());
    };
    let out = substitute(&body, vault);
    let dst = task_dir.join(".claude/settings.local.json");
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(dst, out)
}

pub fn substitute(body: &str, vault: &Path) -> String {
    body.replace("${VAULT}", &vault.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_every_vault_placeholder() {
        let got = substitute(
            r#"["Read(${VAULT}/**)","Write(${VAULT}/a)"]"#,
            Path::new("/v/notes"),
        );
        assert_eq!(got, r#"["Read(/v/notes/**)","Write(/v/notes/a)"]"#);
    }

    #[test]
    fn writes_a_local_override_next_to_the_template() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(
            d.path().join(".claude/settings.json"),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
        )
        .unwrap();
        materialize(d.path(), Path::new("/v/notes")).unwrap();
        let got = std::fs::read_to_string(d.path().join(".claude/settings.local.json")).unwrap();
        assert!(got.contains("Read(/v/notes/**)"));
        assert!(!got.contains("${VAULT}"));
        // The template itself must stay portable.
        let tpl = std::fs::read_to_string(d.path().join(".claude/settings.json")).unwrap();
        assert!(tpl.contains("${VAULT}"));
    }

    #[test]
    fn is_a_no_op_when_the_task_has_no_settings_template() {
        let d = tempfile::tempdir().unwrap();
        materialize(d.path(), Path::new("/v")).unwrap();
        assert!(!d.path().join(".claude/settings.local.json").exists());
    }

    #[test]
    fn rewrites_a_stale_local_override() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(
            d.path().join(".claude/settings.json"),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
        )
        .unwrap();
        std::fs::write(d.path().join(".claude/settings.local.json"), "OLD").unwrap();
        materialize(d.path(), Path::new("/new/vault")).unwrap();
        let got = std::fs::read_to_string(d.path().join(".claude/settings.local.json")).unwrap();
        assert!(got.contains("/new/vault"));
        assert!(!got.contains("OLD"));
    }
}
