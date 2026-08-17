//! The one note a run is aimed at, and the source document behind it.
//!
//! Lifted out of claude-agent's `settings.rs` because it is harness-neutral:
//! resolving `X.note.md` → `X.md` → (if that is a vault mirror) the ORIGINAL on
//! this disk is a fact about the vault, not about which model reads it. What
//! each harness DOES with the scope — a `.claude/settings.local.json` allowlist,
//! a sandbox mode — stays in that harness's own crate.
use crate::mirror::{self, MirrorMeta};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Scope {
    pub note: PathBuf,
    pub source: PathBuf,
    /// The source's directory: the run's real working context. A mirrored
    /// document's neighbours live here, not in the vault.
    pub source_dir: PathBuf,
}

impl Scope {
    /// `X.note.md` sits beside `X.md`. When that `X.md` is a vault mirror whose
    /// original is on this machine, the source is the ORIGINAL — the vault copy
    /// is a snapshot, the original is where the document actually lives. A path
    /// that isn't a sidecar note gets itself as the source, which is harmless:
    /// it only widens reach by one file the run was already pointed at.
    pub fn for_note(vault: &Path, note: &Path, metas: &[MirrorMeta]) -> Scope {
        let s = note.to_string_lossy();
        let beside = s
            .strip_suffix(".note.md")
            .or_else(|| s.strip_suffix(".notes.md"))
            .map(|stem| PathBuf::from(format!("{stem}.md")))
            .unwrap_or_else(|| note.to_path_buf());
        let source = mirror::source_for_mirror(vault, &beside, metas).unwrap_or(beside);
        let source_dir = source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| source.clone());
        Scope {
            note: note.to_path_buf(),
            source,
            source_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(mirror: &str, source: &str) -> MirrorMeta {
        serde_json::from_value(serde_json::json!({
            "mirror": mirror, "deviceId": "d", "deviceName": "n",
            "source": source, "syncedAt": 1, "checksum": "sha256:x",
        }))
        .unwrap()
    }

    #[test]
    fn a_sidecar_note_resolves_to_the_document_beside_it() {
        let s = Scope::for_note(Path::new("/v"), Path::new("/v/docs/a.note.md"), &[]);
        assert_eq!(s.source, PathBuf::from("/v/docs/a.md"));
        assert_eq!(s.source_dir, PathBuf::from("/v/docs"));
    }

    #[test]
    fn the_legacy_notes_suffix_resolves_the_same_way() {
        let s = Scope::for_note(Path::new("/v"), Path::new("/v/docs/a.notes.md"), &[]);
        assert_eq!(s.source, PathBuf::from("/v/docs/a.md"));
    }

    /// The whole point of belief 4: the note lives beside a MIRROR, but the
    /// document actually lives in the project directory it was synced from.
    #[test]
    fn a_mirrors_note_resolves_to_the_original_outside_the_vault() {
        let v = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let original = proj.path().join("DESIGN.md");
        std::fs::write(&original, "# doc").unwrap();

        let metas = vec![meta("Sync/2026-08-17-DESIGN.md", &original.to_string_lossy())];
        let note = v.path().join("Sync/2026-08-17-DESIGN.note.md");
        let s = Scope::for_note(v.path(), &note, &metas);
        assert_eq!(s.source, original);
        assert_eq!(s.source_dir, proj.path());
    }

    #[test]
    fn a_path_that_is_not_a_sidecar_note_gets_itself() {
        let s = Scope::for_note(Path::new("/v"), Path::new("/v/docs/plain.md"), &[]);
        assert_eq!(s.source, PathBuf::from("/v/docs/plain.md"));
    }
}
