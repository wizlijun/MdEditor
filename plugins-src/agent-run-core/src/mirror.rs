//! Resolving a vault mirror back to the ORIGINAL file it was synced from.
//!
//! A document the user reads outside the vault gets mirrored into it the moment
//! they annotate it (belief 4: marks belong to the vault, not to a path), and
//! the sidecar `.note.md` lands next to the MIRROR. Pointing the agent at that
//! mirror puts it on an island: a snapshot with none of the neighbours — sibling
//! docs, images, the project's own conventions — that give the document meaning.
//! So a run about such a note reads the original, in its original directory.
//!
//! This process does not know its own device id (the host generates it), so the
//! resolution is by EXISTENCE, not by device: among every device's recorded
//! source for this mirror, take one that is actually on this disk. A path from
//! another machine simply isn't there.
use serde::Deserialize;
use std::path::{Path, PathBuf};

const META_SUBDIR: &str = ".notemd/mirrors";

/// One mirror-meta file, git-synced under `{vault}/.notemd/mirrors/`. Written by
/// the host (`src-tauri/src/sotvault/mirror_meta.rs`) in camelCase. Unknown keys
/// are ignored — a newer host may add fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorMeta {
    /// Vault-relative path of the mirror.
    pub mirror: String,
    /// Absolute path of the original on the device that recorded it.
    pub source: String,
}

/// Every readable meta in the vault. A corrupt or foreign file is skipped, never
/// fatal — the consumer's tolerance obligation.
pub fn read_metas(vault: &Path) -> Vec<MirrorMeta> {
    let dir = vault.join(META_SUBDIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<MirrorMeta> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| std::fs::read_to_string(&p).ok())
        .filter_map(|s| serde_json::from_str::<MirrorMeta>(&s).ok())
        .filter(|m| !m.mirror.is_empty() && !m.source.is_empty())
        .collect();
    // Stable order regardless of directory iteration order.
    out.sort_by(|a, b| (&a.mirror, &a.source).cmp(&(&b.mirror, &b.source)));
    out
}

/// The original file `mirror_abs` was synced from, on THIS machine.
///
/// Requires the candidate to exist locally and to carry the same file name as
/// the mirror's own stem-bearing name would suggest — a mirror is `<date>-<name>.md`,
/// so the check is "the mirror's file name ends with the source's file name".
/// That keeps a stale path from another machine, which happens to exist here,
/// from being mistaken for this document's source.
pub fn source_for_mirror(vault: &Path, mirror_abs: &Path, metas: &[MirrorMeta]) -> Option<PathBuf> {
    let want = mirror_abs.file_name()?.to_string_lossy().to_string();
    metas
        .iter()
        .filter(|m| vault.join(&m.mirror) == mirror_abs)
        .map(|m| PathBuf::from(&m.source))
        .find(|src| {
            src.is_file()
                && src
                    .file_name()
                    .map(|n| want.ends_with(&*n.to_string_lossy()))
                    .unwrap_or(false)
        })
}

/// Directories of every locally-present original, deduped and sorted. A
/// whole-vault sweep grants Read on these so each note's own source is
/// reachable, whichever mirror it belongs to.
pub fn local_source_dirs(metas: &[MirrorMeta]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = metas
        .iter()
        .map(|m| PathBuf::from(&m.source))
        .filter(|p| p.is_file())
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_meta(vault: &Path, name: &str, mirror: &str, source: &str) {
        let dir = vault.join(META_SUBDIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(name),
            format!(
                r#"{{"mirror":"{mirror}","deviceId":"d","deviceName":"n","source":"{source}","syncedAt":1,"checksum":"sha256:x"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn reads_every_meta_and_skips_the_unreadable_ones() {
        let v = tempfile::tempdir().unwrap();
        write_meta(v.path(), "a.json", "Sync/a.md", "/src/a.md");
        write_meta(v.path(), "b.json", "Sync/b.md", "/src/b.md");
        std::fs::write(v.path().join(META_SUBDIR).join("bad.json"), "{ not json").unwrap();
        std::fs::write(v.path().join(META_SUBDIR).join("note.txt"), "ignored").unwrap();
        let metas = read_metas(v.path());
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].mirror, "Sync/a.md");
    }

    #[test]
    fn no_meta_dir_is_not_an_error() {
        let v = tempfile::tempdir().unwrap();
        assert!(read_metas(v.path()).is_empty());
    }

    #[test]
    fn resolves_the_mirror_to_the_local_original() {
        let v = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let src = proj.path().join("DESIGN.CN.md");
        std::fs::write(&src, "# doc").unwrap();
        write_meta(
            v.path(),
            "m.json",
            "Sync/2026-08-04-DESIGN.CN.md",
            &src.to_string_lossy(),
        );
        let metas = read_metas(v.path());
        let mirror = v.path().join("Sync/2026-08-04-DESIGN.CN.md");
        assert_eq!(source_for_mirror(v.path(), &mirror, &metas), Some(src));
    }

    #[test]
    fn a_source_from_another_device_does_not_resolve() {
        let v = tempfile::tempdir().unwrap();
        write_meta(
            v.path(),
            "m.json",
            "Sync/a.md",
            "/Users/someone-else/notes/a.md",
        );
        let metas = read_metas(v.path());
        let mirror = v.path().join("Sync/a.md");
        assert_eq!(source_for_mirror(v.path(), &mirror, &metas), None);
    }

    #[test]
    fn a_local_path_with_a_different_name_is_not_mistaken_for_the_source() {
        let v = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let other = proj.path().join("unrelated.md");
        std::fs::write(&other, "x").unwrap();
        write_meta(
            v.path(),
            "m.json",
            "Sync/2026-08-04-a.md",
            &other.to_string_lossy(),
        );
        let metas = read_metas(v.path());
        let mirror = v.path().join("Sync/2026-08-04-a.md");
        assert_eq!(source_for_mirror(v.path(), &mirror, &metas), None);
    }

    #[test]
    fn a_mirror_with_no_meta_resolves_to_nothing() {
        let v = tempfile::tempdir().unwrap();
        let mirror = v.path().join("Sync/ghost.md");
        assert_eq!(source_for_mirror(v.path(), &mirror, &[]), None);
    }

    #[test]
    fn source_dirs_are_deduped_sorted_and_local_only() {
        let v = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        for n in ["a.md", "b.md"] {
            std::fs::write(proj.path().join(n), "x").unwrap();
            write_meta(
                v.path(),
                &format!("{n}.json"),
                &format!("Sync/{n}"),
                &proj.path().join(n).to_string_lossy(),
            );
        }
        write_meta(v.path(), "gone.json", "Sync/gone.md", "/nowhere/gone.md");
        let dirs = local_source_dirs(&read_metas(v.path()));
        assert_eq!(dirs, vec![proj.path().to_path_buf()]);
    }
}
