//! A template's `.claude/settings.json` stays portable by writing `${VAULT}`
//! instead of a machine path. Before each run the placeholders are substituted
//! into `.claude/settings.local.json` — Claude Code's own local override layer,
//! already gitignored on the vault side.
//!
//! A run aimed at ONE note uses a narrower policy, `settings.scoped.json`, if
//! the template ships one. That is what actually confines the run: telling a
//! model in its prompt to look at a single file does not stop it from grepping
//! the vault, and it did exactly that until the permissions said otherwise.
use std::path::{Path, PathBuf};

/// The one note a run is aimed at, and the source document beside it — the
/// protocol needs both, since a question's `line::` points into the source.
#[derive(Debug, Clone)]
pub struct Scope {
    pub note: PathBuf,
    pub source: PathBuf,
}

impl Scope {
    /// `X.note.md` sits beside `X.md`. A path that isn't a sidecar note gets
    /// itself as the source, which is harmless: it only widens Read by one file
    /// the run was already pointed at.
    pub fn for_note(note: &Path) -> Scope {
        let s = note.to_string_lossy();
        let source = s
            .strip_suffix(".note.md")
            .or_else(|| s.strip_suffix(".notes.md"))
            .map(|stem| PathBuf::from(format!("{stem}.md")))
            .unwrap_or_else(|| note.to_path_buf());
        Scope {
            note: note.to_path_buf(),
            source,
        }
    }
}

/// Generate `.claude/settings.local.json`. A task without any settings template
/// is fine — silently skipped.
pub fn materialize(task_dir: &Path, vault: &Path, scope: Option<&Scope>) -> std::io::Result<()> {
    let scoped = task_dir.join(".claude/settings.scoped.json");
    let src = match scope {
        Some(_) if scoped.is_file() => scoped,
        _ => task_dir.join(".claude/settings.json"),
    };
    let Ok(body) = std::fs::read_to_string(&src) else {
        return Ok(());
    };
    let out = substitute(&body, vault, scope);
    let dst = task_dir.join(".claude/settings.local.json");
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(dst, out)
}

pub fn substitute(body: &str, vault: &Path, scope: Option<&Scope>) -> String {
    let mut out = body.replace("${VAULT}", &vault.to_string_lossy());
    if let Some(s) = scope {
        out = out
            .replace("${NOTE}", &s.note.to_string_lossy())
            .replace("${SOURCE}", &s.source.to_string_lossy());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope::for_note(Path::new("/v/docs/a.note.md"))
    }

    fn write_templates(dir: &Path, broad: &str, narrow: Option<&str>) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), broad).unwrap();
        if let Some(n) = narrow {
            std::fs::write(dir.join(".claude/settings.scoped.json"), n).unwrap();
        }
    }

    fn local(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(".claude/settings.local.json")).unwrap()
    }

    #[test]
    fn a_sidecar_note_resolves_to_its_source_document() {
        assert_eq!(scope().source, PathBuf::from("/v/docs/a.md"));
        assert_eq!(
            Scope::for_note(Path::new("/v/b.notes.md")).source,
            PathBuf::from("/v/b.md")
        );
        // Not a sidecar name: no second file is opened up.
        let plain = Scope::for_note(Path::new("/v/c.md"));
        assert_eq!(plain.source, plain.note);
    }

    #[test]
    fn replaces_every_placeholder() {
        let got = substitute(
            r#"["Read(${VAULT}/**)","Read(${NOTE})","Read(${SOURCE})"]"#,
            Path::new("/v"),
            Some(&scope()),
        );
        assert_eq!(
            got,
            r#"["Read(/v/**)","Read(/v/docs/a.note.md)","Read(/v/docs/a.md)"]"#
        );
    }

    #[test]
    fn a_note_run_gets_the_narrow_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"],"deny":["Grep"]}"#),
        );
        materialize(d.path(), Path::new("/v"), Some(&scope())).unwrap();
        let got = local(d.path());
        assert!(got.contains("Read(/v/docs/a.note.md)"), "got {got}");
        assert!(got.contains("Grep"), "the deny list must come through: {got}");
        assert!(!got.contains("Read(/v/**)"), "must not fall back to vault-wide");
    }

    #[test]
    fn a_whole_vault_run_keeps_the_broad_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"]}"#),
        );
        materialize(d.path(), Path::new("/v"), None).unwrap();
        assert!(local(d.path()).contains("Read(/v/**)"));
    }

    #[test]
    fn a_note_run_falls_back_when_the_template_has_no_narrow_policy() {
        let d = tempfile::tempdir().unwrap();
        write_templates(d.path(), r#"{"allow":["Read(${VAULT}/**)"]}"#, None);
        materialize(d.path(), Path::new("/v"), Some(&scope())).unwrap();
        assert!(local(d.path()).contains("Read(/v/**)"));
    }

    #[test]
    fn the_portable_templates_are_never_rewritten() {
        let d = tempfile::tempdir().unwrap();
        write_templates(
            d.path(),
            r#"{"allow":["Read(${VAULT}/**)"]}"#,
            Some(r#"{"allow":["Read(${NOTE})"]}"#),
        );
        materialize(d.path(), Path::new("/v"), Some(&scope())).unwrap();
        for f in [".claude/settings.json", ".claude/settings.scoped.json"] {
            let t = std::fs::read_to_string(d.path().join(f)).unwrap();
            assert!(t.contains("${"), "{f} lost its placeholders");
        }
    }

    #[test]
    fn is_a_no_op_when_the_task_has_no_settings_template() {
        let d = tempfile::tempdir().unwrap();
        materialize(d.path(), Path::new("/v"), None).unwrap();
        assert!(!d.path().join(".claude/settings.local.json").exists());
    }

    #[test]
    fn rewrites_a_stale_local_override() {
        let d = tempfile::tempdir().unwrap();
        write_templates(d.path(), r#"{"allow":["Read(${VAULT}/**)"]}"#, None);
        std::fs::write(d.path().join(".claude/settings.local.json"), "OLD").unwrap();
        materialize(d.path(), Path::new("/new/vault"), None).unwrap();
        let got = local(d.path());
        assert!(got.contains("/new/vault"));
        assert!(!got.contains("OLD"));
    }
}
