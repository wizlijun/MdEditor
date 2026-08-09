//! Plugin installer core (子项目③ Task 1): download → verify → unpack →
//! atomic swap → rollback. Pure functions with no `AppHandle`, so the whole
//! pipeline is testable against a `tempdir`. GUI (market commands) and CLI
//! share this layer.
//!
//! **Security contract** (spec §3, ②安全评审):
//! 1. sha256(pkg_bytes) must equal the index-recorded hex (integrity).
//! 2. The detached minisign signature must verify against pkg_bytes with the
//!    hard-coded registry public key (authenticity). Neither check is optional
//!    and there is no path that stages files before both pass.
//! 3. Unpacking rejects any entry whose normalized path escapes `stage_dir`
//!    (Zip-slip / path traversal defence) — we rely on `zip`'s
//!    [`ZipFile::enclosed_name`], which returns `None` for unsafe names.
//! 4. The staged manifest is validated (engines/id/binary|ui) and its `id`
//!    must equal the caller's expected id before commit.
//! 5. Decompression is bounded: no per-entry buffer is pre-reserved to the
//!    attacker-declared `size()`, and the cumulative unpacked total is capped at
//!    [`MAX_UNPACKED_BYTES`] via a streaming copy so a compression bomb can't
//!    exhaust disk/memory (②安全评审 V1+V2).
//!
//! Package format: `.notemdpkg` = a zip archive containing `manifest.json`
//! plus `bin/…` and/or `ui/…`.

use plugin_protocol::{validate_manifest, ManifestV2};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

/// Hard ceiling on the *decompressed* total across all entries of one package
/// (200 MiB). Unpacking aborts once the cumulative written bytes would exceed
/// this, defeating a compression bomb whose declared sizes or deflate ratio are
/// hostile (②安全评审 V1+V2).
pub const MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;

/// All the ways installation can fail. Each variant maps to a user-facing
/// string via [`std::fmt::Display`].
#[derive(Debug)]
pub enum InstallError {
    /// sha256(pkg) != the expected hex from the registry index.
    Hash,
    /// minisign signature did not verify against the package bytes.
    Signature,
    /// Zip decode / traversal-guard / IO during unpack. Carries a detail.
    Unpack(String),
    /// manifest.json missing, malformed, or failed `validate_manifest`.
    Manifest(String),
    /// Staged manifest id != the id the caller asked to install.
    IdMismatch,
    /// Filesystem error during commit/uninstall/rollback.
    Io(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Hash => write!(f, "package hash mismatch (sha256)"),
            InstallError::Signature => write!(f, "package signature verification failed"),
            InstallError::Unpack(d) => write!(f, "failed to unpack package: {d}"),
            InstallError::Manifest(d) => write!(f, "invalid plugin manifest: {d}"),
            InstallError::IdMismatch => write!(f, "package id does not match requested plugin"),
            InstallError::Io(d) => write!(f, "filesystem error: {d}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Constant-time-ish comparison of two byte slices. Avoids the early-return of
/// `==` so a timing side-channel cannot leak how many leading hash bytes match.
/// (The hash is not itself secret, but the signature guarantee should not lean
/// on a data-dependent comparison here either.)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Verify integrity + authenticity, then unpack to `stage_dir` and validate the
/// manifest. On success returns the parsed [`ManifestV2`]; the caller then calls
/// [`commit_install`]. On any failure `stage_dir` may contain partial output —
/// callers should stage into a fresh tempdir and discard it on error.
///
/// * `pkg_bytes`   — the raw `.notemdpkg` (zip) bytes.
/// * `sig`         — the full `.minisig` file text (the whole detached sig).
/// * `sha256_hex`  — expected lowercase hex sha256 from the registry index.
/// * `pubkey_b64`  — the registry's minisign public key (base64, no comment).
/// * `expected_id` — the plugin id the caller intends to install.
/// * `host_version`— the running host version for the engines check.
/// * `stage_dir`   — an existing empty directory to unpack into.
pub fn verify_and_stage(
    pkg_bytes: &[u8],
    sig: &str,
    sha256_hex: &str,
    pubkey_b64: &str,
    expected_id: &str,
    host_version: &str,
    stage_dir: &Path,
) -> Result<ManifestV2, InstallError> {
    // (1) Integrity: sha256 must match the index-recorded hex.
    let digest = Sha256::digest(pkg_bytes);
    let expected = hex::decode(sha256_hex.trim()).map_err(|_| InstallError::Hash)?;
    if !ct_eq(&digest, &expected) {
        return Err(InstallError::Hash);
    }

    // (2) Authenticity: detached minisign signature over the package bytes.
    // minisign (>=0.6 / rsign2) hashes the message with BLAKE2b before signing
    // ("prehashed"); `Signature` carries the algorithm tag, so we let
    // `minisign-verify` pick the right mode by trying prehashed first and then
    // the legacy Ed25519-over-raw-bytes mode. A tampered package fails both.
    let public_key =
        minisign_verify::PublicKey::from_base64(pubkey_b64).map_err(|_| InstallError::Signature)?;
    let signature = minisign_verify::Signature::decode(sig).map_err(|_| InstallError::Signature)?;
    let verified = public_key.verify(pkg_bytes, &signature, false).is_ok()
        || public_key.verify(pkg_bytes, &signature, true).is_ok();
    if !verified {
        return Err(InstallError::Signature);
    }

    // (3) Unpack with a per-entry traversal guard and a cumulative size cap.
    unpack_zip(pkg_bytes, stage_dir, MAX_UNPACKED_BYTES)?;

    // (4) Read + validate the staged manifest.
    let manifest_path = stage_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| InstallError::Manifest(format!("manifest.json: {e}")))?;
    let manifest: ManifestV2 =
        serde_json::from_str(&text).map_err(|e| InstallError::Manifest(format!("manifest.json: {e}")))?;
    validate_manifest(&manifest, host_version).map_err(InstallError::Manifest)?;

    // (5) The staged id must be exactly what the caller intended.
    if manifest.id != expected_id {
        return Err(InstallError::IdMismatch);
    }

    Ok(manifest)
}

/// Unzip `pkg_bytes` into `stage_dir`, rejecting any entry whose path is not
/// safely contained within `stage_dir` (Zip-slip defence) and aborting if the
/// cumulative *decompressed* total across all entries would exceed
/// `max_unpacked` (compression-bomb defence). The declared `entry.size()` is
/// never trusted for allocation: every entry is streamed in fixed chunks and
/// the running budget is checked before each chunk is written.
fn unpack_zip(pkg_bytes: &[u8], stage_dir: &Path, max_unpacked: u64) -> Result<(), InstallError> {
    let reader = std::io::Cursor::new(pkg_bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| InstallError::Unpack(e.to_string()))?;

    let mut total_written: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| InstallError::Unpack(e.to_string()))?;

        // `enclosed_name` returns None for absolute paths, `..` components, or
        // anything else that would escape the destination. Reject those.
        let rel = match entry.enclosed_name() {
            Some(p) => p,
            None => {
                return Err(InstallError::Unpack(format!(
                    "unsafe path in package: '{}'",
                    entry.name()
                )))
            }
        };

        // Belt-and-suspenders: independently confirm the joined path stays
        // inside stage_dir after normalizing, so a future zip-crate regression
        // can't reopen the hole.
        if !is_contained(stage_dir, &rel) {
            return Err(InstallError::Unpack(format!(
                "path escapes staging dir: '{}'",
                entry.name()
            )));
        }

        let out_path = stage_dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| InstallError::Unpack(e.to_string()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| InstallError::Unpack(e.to_string()))?;
        }

        // Stream the entry into the file in bounded chunks, checking the
        // cumulative budget before writing each chunk. `entry.size()` (the
        // attacker-declared size) is deliberately NOT used to pre-reserve.
        total_written = copy_entry_capped(&mut entry, &out_path, total_written, max_unpacked)?;

        // Preserve the executable bit so shipped binaries are runnable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(
                    &out_path,
                    std::fs::Permissions::from_mode(mode),
                );
            }
        }
    }
    Ok(())
}

/// Stream one zip entry into `out_path`, enforcing that `already_written` plus
/// this entry's decompressed bytes never exceeds `max_unpacked`. Returns the new
/// running total on success; on overflow returns `InstallError::Unpack` (and
/// leaves the partial file for the caller's tempdir to discard).
fn copy_entry_capped(
    entry: &mut impl std::io::Read,
    out_path: &Path,
    already_written: u64,
    max_unpacked: u64,
) -> Result<u64, InstallError> {
    const CHUNK: usize = 64 * 1024;
    let mut file = std::fs::File::create(out_path).map_err(|e| InstallError::Unpack(e.to_string()))?;
    let mut buf = [0u8; CHUNK];
    let mut total = already_written;
    loop {
        let n = entry.read(&mut buf).map_err(|e| InstallError::Unpack(e.to_string()))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_unpacked {
            return Err(InstallError::Unpack(format!(
                "package expands beyond {} MiB",
                max_unpacked / (1024 * 1024)
            )));
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| InstallError::Unpack(e.to_string()))?;
    }
    Ok(total)
}

/// True iff `base.join(rel)` normalizes to a path still rooted at `base` and the
/// relative part contains no `..`/root/prefix components. `rel` must be relative.
fn is_contained(base: &Path, rel: &Path) -> bool {
    if rel.is_absolute() {
        return false;
    }
    let mut depth: i32 = 0;
    for comp in rel.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            // RootDir / Prefix ⇒ absolute-ish, reject.
            _ => return false,
        }
        if let Component::Normal(_) = comp {
            depth += 1;
        }
    }
    // `base` is only used to keep the signature intent-revealing; the traversal
    // math above is what actually guarantees containment.
    let _ = base;
    true
}

/// Atomically install an already-staged plugin tree to
/// `<root>/<id>/<version>/` and point `<root>/<id>/current` at `<version>`.
///
/// Steps: if the version dir already exists it is removed first (reinstall is
/// idempotent). The staged tree is copied into place. Then a fresh `current.tmp`
/// symlink is created and renamed over `current` — on unix `rename(2)` is atomic
/// so a reader either sees the old or the new target, never a half state. The
/// previous `current` is untouched until the rename succeeds, so a failure
/// before that point leaves the prior install intact.
pub fn commit_install(
    root: &Path,
    id: &str,
    version: &str,
    staged: &Path,
) -> Result<(), InstallError> {
    let id_dir = root.join(id);
    let version_dir = id_dir.join(version);

    std::fs::create_dir_all(&id_dir).map_err(io)?;

    // Idempotent reinstall: clear any prior copy of this exact version.
    if version_dir.exists() {
        std::fs::remove_dir_all(&version_dir).map_err(io)?;
    }
    copy_tree(staged, &version_dir)?;

    // Atomically point current → version via a temp symlink + rename.
    repoint_current(&id_dir, version)?;
    Ok(())
}

/// Repoint `<id_dir>/current` at `<version>` atomically. `version` must already
/// exist as `<id_dir>/<version>`.
fn repoint_current(id_dir: &Path, version: &str) -> Result<(), InstallError> {
    let version_dir = id_dir.join(version);
    if !version_dir.exists() {
        return Err(InstallError::Io(format!(
            "version dir '{}' does not exist",
            version_dir.display()
        )));
    }
    let current = id_dir.join("current");
    let tmp = id_dir.join("current.tmp");
    // Clean any leftover tmp from a crashed prior run.
    let _ = std::fs::remove_file(&tmp);

    #[cfg(unix)]
    {
        // Relative target so the tree is relocatable.
        crate::platform::link_dir(std::path::Path::new(version), &tmp).map_err(io)?;
    }
    #[cfg(not(unix))]
    {
        // Absolute: a Windows junction (the no-privilege fallback inside
        // `link_dir`) cannot name a relative target.
        crate::platform::link_dir(&version_dir, &tmp).map_err(io)?;
    }

    // rename over the existing `current` (atomic replace on unix).
    std::fs::rename(&tmp, &current).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        InstallError::Io(e.to_string())
    })?;
    Ok(())
}

/// Remove the whole `<root>/<id>/` install tree. When `!keep_data` also remove
/// the plugin's data dir `<data_root>/plugin_data/<id>`.
pub fn uninstall(
    root: &Path,
    id: &str,
    keep_data: bool,
    data_root: &Path,
) -> Result<(), InstallError> {
    let id_dir = root.join(id);
    if id_dir.exists() {
        std::fs::remove_dir_all(&id_dir).map_err(io)?;
    }
    if !keep_data {
        let data_dir = data_root.join("plugin_data").join(id);
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir).map_err(io)?;
        }
    }
    Ok(())
}

/// Repoint `current` back to an already-installed `to_version` (upgrade abort).
/// The target version dir must exist.
pub fn rollback(root: &Path, id: &str, to_version: &str) -> Result<(), InstallError> {
    let id_dir = root.join(id);
    repoint_current(&id_dir, to_version)
}

/// Recursively copy `src` → `dst` (dst created). Files, dirs, and — on unix —
/// symlinks are preserved. Executable bits ride along via the raw copy.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), InstallError> {
    std::fs::create_dir_all(dst).map_err(io)?;
    for entry in std::fs::read_dir(src).map_err(io)? {
        let entry = entry.map_err(io)?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type().map_err(io)?;
        if ft.is_dir() {
            copy_tree(&from, &to)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from).map_err(io)?;
                std::os::unix::fs::symlink(&target, &to).map_err(io)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::copy(&from, &to).map_err(io)?;
            }
        } else {
            std::fs::copy(&from, &to).map_err(io)?;
        }
    }
    Ok(())
}

fn io(e: std::io::Error) -> InstallError {
    InstallError::Io(e.to_string())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;

    /// The minisign PUBLIC key for the committed test fixtures. The matching
    /// PRIVATE key is NOT in the repo (it was a throwaway used once to sign the
    /// fixture package via the `minisign` CLI — see the task report). This is
    /// the base64 key line only (no `untrusted comment:` prefix), as
    /// `PublicKey::from_base64` expects.
    const TEST_PUBKEY_B64: &str = include_str!("../../tests/fixtures/pkg/test.pub.b64");

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pkg")
    }

    fn read_fixture(name: &str) -> Vec<u8> {
        std::fs::read(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
    }

    fn read_fixture_str(name: &str) -> String {
        String::from_utf8(read_fixture(name)).unwrap()
    }

    const FIXTURE_ID: &str = "test.installer-fixture";
    const FIXTURE_VERSION: &str = "1.0.0";
    const HOST: &str = "1.0.0";

    fn pubkey() -> &'static str {
        TEST_PUBKEY_B64.trim()
    }

    /// sha256 hex of the fixture package.
    fn fixture_sha() -> String {
        hex::encode(Sha256::digest(read_fixture("fixture.notemdpkg")))
    }

    // ── verify_and_stage: happy path ──────────────────────────────────────

    #[test]
    fn happy_stage_returns_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let m = verify_and_stage(
            &read_fixture("fixture.notemdpkg"),
            &read_fixture_str("fixture.notemdpkg.minisig"),
            &fixture_sha(),
            pubkey(),
            FIXTURE_ID,
            HOST,
            dir.path(),
        )
        .expect("happy path should verify+stage");
        assert_eq!(m.id, FIXTURE_ID);
        assert_eq!(m.version, FIXTURE_VERSION);
        assert!(dir.path().join("manifest.json").is_file());
        assert!(dir.path().join("ui/index.html").is_file());
    }

    // ── verify_and_stage: sha mismatch ────────────────────────────────────

    #[test]
    fn sha_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let bad = "0".repeat(64);
        let err = verify_and_stage(
            &read_fixture("fixture.notemdpkg"),
            &read_fixture_str("fixture.notemdpkg.minisig"),
            &bad,
            pubkey(),
            FIXTURE_ID,
            HOST,
            dir.path(),
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Hash), "got {err:?}");
    }

    // ── verify_and_stage: bad signature ───────────────────────────────────

    #[test]
    fn bad_signature_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // Tamper with the package bytes: correct-looking sig, wrong content.
        let mut pkg = read_fixture("fixture.notemdpkg");
        let last = pkg.len() - 1;
        pkg[last] ^= 0xff;
        let err = verify_and_stage(
            &pkg,
            &read_fixture_str("fixture.notemdpkg.minisig"),
            // recompute sha so it passes the hash gate and we reach the sig gate
            &hex::encode(Sha256::digest(&pkg)),
            pubkey(),
            FIXTURE_ID,
            HOST,
            dir.path(),
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Signature), "got {err:?}");
    }

    // ── verify_and_stage: id mismatch ─────────────────────────────────────

    #[test]
    fn id_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = verify_and_stage(
            &read_fixture("fixture.notemdpkg"),
            &read_fixture_str("fixture.notemdpkg.minisig"),
            &fixture_sha(),
            pubkey(),
            "test.some-other-plugin",
            HOST,
            dir.path(),
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::IdMismatch), "got {err:?}");
    }

    // ── verify_and_stage: engines unsatisfied ─────────────────────────────

    #[test]
    fn engines_unsatisfied_rejected() {
        // The fixture manifest declares engines ">=0.1.0"; a host of "0.0.1"
        // fails the engines check inside validate_manifest, surfacing as
        // InstallError::Manifest (engines are checked only after sig+hash pass).
        let dir = tempfile::tempdir().unwrap();
        let err = verify_and_stage(
            &read_fixture("fixture.notemdpkg"),
            &read_fixture_str("fixture.notemdpkg.minisig"),
            &fixture_sha(),
            pubkey(),
            FIXTURE_ID,
            "0.0.1",
            dir.path(),
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Manifest(_)), "got {err:?}");
    }

    // ── verify_and_stage: traversal entry (synthesized, self-signed key) ───
    //
    // We can't sign a malicious zip with the real (discarded) private key, so
    // this test drives `unpack_zip` directly — the exact function the traversal
    // guard lives in — with a zip that carries a `../escape` entry.

    fn zip_with_entry(name: &str, body: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.start_file(name, SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn traversal_entry_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let evil = zip_with_entry("../escape.txt", b"pwned");
        let err = unpack_zip(&evil, dir.path(), MAX_UNPACKED_BYTES).unwrap_err();
        assert!(matches!(err, InstallError::Unpack(_)), "got {err:?}");
        // Nothing escaped.
        assert!(!dir.path().parent().unwrap().join("escape.txt").exists());
    }

    #[test]
    fn absolute_entry_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // zip stores forward-slash names; a leading slash ⇒ absolute.
        let evil = zip_with_entry("/etc/pwned", b"x");
        // enclosed_name() strips a leading slash to a relative path on some
        // platforms; either way the guard must not write outside stage_dir.
        let _ = unpack_zip(&evil, dir.path(), MAX_UNPACKED_BYTES);
        assert!(!Path::new("/etc/pwned").exists());
    }

    // ── unpack_zip: decompression-bomb cumulative cap (②安全评审 V1+V2) ──────
    //
    // The declared per-entry size is never trusted; the guard is a running total
    // checked before each chunk is written. We prove it by unpacking a package
    // whose *actual* decompressed total exceeds a tiny test-only cap and asserting
    // the Unpack error fires. `copy_entry_capped` takes the cap as a param so the
    // test can pass a value far below the 200 MiB production ceiling.

    /// Build a zip with several stored (uncompressed) entries so the decompressed
    /// total is predictable regardless of the deflate ratio.
    fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn cumulative_cap_exceeded_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // Three 40 KiB entries = 120 KiB decompressed; cap at 100 KiB.
        let body = vec![b'A'; 40 * 1024];
        let pkg = zip_with_entries(&[
            ("a.bin", &body),
            ("b.bin", &body),
            ("c.bin", &body),
        ]);
        let cap: u64 = 100 * 1024;
        let err = unpack_zip(&pkg, dir.path(), cap).unwrap_err();
        match err {
            InstallError::Unpack(msg) => assert!(msg.contains("expands beyond"), "got {msg}"),
            other => panic!("expected Unpack, got {other:?}"),
        }
    }

    #[test]
    fn under_cap_unpacks_fine() {
        let dir = tempfile::tempdir().unwrap();
        let body = vec![b'A'; 40 * 1024];
        let pkg = zip_with_entries(&[("a.bin", &body), ("b.bin", &body)]);
        // 80 KiB total, cap at 100 KiB → succeeds.
        unpack_zip(&pkg, dir.path(), 100 * 1024).expect("under-cap package unpacks");
        assert!(dir.path().join("a.bin").is_file());
        assert!(dir.path().join("b.bin").is_file());
    }

    // ── unpack_zip: symlink entry (S_IFLNK) must not become a live symlink ──
    //
    // A malicious package may carry an entry whose unix mode marks it a symlink
    // (S_IFLNK, 0o120000) with target-path content like `../../etc/evil`. Our
    // unpacker writes every entry as a regular file (it never calls symlink()),
    // so the "link" lands as an inert regular file inside the stage dir and its
    // path (validated by enclosed_name + is_contained) cannot escape. (②安全评审 V4)

    /// Build a zip with one entry carrying an explicit unix mode.
    fn zip_with_unix_mode(name: &str, body: &[u8], mode: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default().unix_permissions(mode);
            w.start_file(name, opts).unwrap();
            w.write_all(body).unwrap();
            w.finish().unwrap();
        }
        buf
    }

    #[test]
    fn symlink_entry_becomes_regular_file_no_escape() {
        const S_IFLNK: u32 = 0o120000;
        let dir = tempfile::tempdir().unwrap();
        // Legal in-tree name; the danger would be treating the *content* as a
        // symlink target. Content is a classic traversal target string.
        let pkg = zip_with_unix_mode("evil-link", b"../../etc/evil", S_IFLNK | 0o777);
        unpack_zip(&pkg, dir.path(), MAX_UNPACKED_BYTES).expect("unpack");

        let out = dir.path().join("evil-link");
        // The entry materialized as a *regular* file, NOT a symlink.
        let meta = std::fs::symlink_metadata(&out).expect("entry exists");
        assert!(!meta.file_type().is_symlink(), "S_IFLNK entry must not become a live symlink");
        assert!(meta.file_type().is_file(), "S_IFLNK entry must be a regular file");
        // Its bytes are the literal target string, inert — nothing followed it.
        assert_eq!(std::fs::read(&out).unwrap(), b"../../etc/evil");
        // Nothing escaped the stage dir.
        assert!(!Path::new("/etc/evil").exists());
        assert!(!dir.path().parent().unwrap().join("etc").join("evil").exists());
    }

    #[test]
    fn is_contained_math() {
        let base = Path::new("/tmp/stage");
        assert!(is_contained(base, Path::new("ui/index.html")));
        assert!(is_contained(base, Path::new("a/b/c")));
        assert!(is_contained(base, Path::new("a/../b"))); // net depth stays >=0
        assert!(!is_contained(base, Path::new("../escape")));
        assert!(!is_contained(base, Path::new("a/../../escape")));
        assert!(!is_contained(base, Path::new("/abs")));
    }

    // ── commit / current symlink ──────────────────────────────────────────

    fn stage_the_fixture(stage: &Path) {
        verify_and_stage(
            &read_fixture("fixture.notemdpkg"),
            &read_fixture_str("fixture.notemdpkg.minisig"),
            &fixture_sha(),
            pubkey(),
            FIXTURE_ID,
            HOST,
            stage,
        )
        .expect("stage");
    }

    /// `<id_dir>/current` resolves to `<id_dir>/<version>`.
    ///
    /// Compares the *resolved* target rather than the literal link body: unix
    /// stores a relative `"1.0.0"` so the tree stays relocatable, while a
    /// Windows junction (the no-privilege fallback in `platform::link_dir`) can
    /// only store an absolute path. Both must land on the same directory, and
    /// that is the property these tests are actually about.
    fn assert_current_points_at(id_dir: &Path, version: &str) {
        let current = id_dir.join("current");
        let target = std::fs::read_link(&current).unwrap();
        let resolved = if target.is_absolute() { target } else { id_dir.join(target) };
        assert_eq!(
            std::fs::canonicalize(&resolved).unwrap(),
            std::fs::canonicalize(id_dir.join(version)).unwrap(),
        );
    }

    #[test]
    fn commit_creates_version_dir_and_current() {
        let root_dir = tempfile::tempdir().unwrap();
        let stage_dir = tempfile::tempdir().unwrap();
        stage_the_fixture(stage_dir.path());
        let root = root_dir.path();

        commit_install(root, FIXTURE_ID, FIXTURE_VERSION, stage_dir.path()).unwrap();

        let version_dir = root.join(FIXTURE_ID).join(FIXTURE_VERSION);
        assert!(version_dir.join("manifest.json").is_file());
        assert!(version_dir.join("ui/index.html").is_file());

        let current = root.join(FIXTURE_ID).join("current");
        assert!(current.exists());
        // current → version resolves to the manifest.
        assert!(current.join("manifest.json").is_file());
        assert_current_points_at(&root.join(FIXTURE_ID), FIXTURE_VERSION);
    }

    #[test]
    fn commit_is_idempotent_reinstall_same_version() {
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();

        for _ in 0..2 {
            let stage_dir = tempfile::tempdir().unwrap();
            stage_the_fixture(stage_dir.path());
            commit_install(root, FIXTURE_ID, FIXTURE_VERSION, stage_dir.path())
                .expect("reinstall same version should succeed");
        }
        let current = root.join(FIXTURE_ID).join("current");
        assert!(current.join("manifest.json").is_file());
    }

    // ── uninstall ─────────────────────────────────────────────────────────

    #[test]
    fn uninstall_removes_tree_and_data() {
        let root_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();
        let data_root = data_dir.path();

        let stage_dir = tempfile::tempdir().unwrap();
        stage_the_fixture(stage_dir.path());
        commit_install(root, FIXTURE_ID, FIXTURE_VERSION, stage_dir.path()).unwrap();

        // Fake a data dir the plugin wrote.
        let plugin_data = data_root.join("plugin_data").join(FIXTURE_ID);
        std::fs::create_dir_all(&plugin_data).unwrap();
        std::fs::write(plugin_data.join("db"), b"state").unwrap();

        uninstall(root, FIXTURE_ID, false, data_root).unwrap();
        assert!(!root.join(FIXTURE_ID).exists());
        assert!(!plugin_data.exists());
    }

    #[test]
    fn uninstall_keep_data_preserves_data_dir() {
        let root_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();
        let data_root = data_dir.path();

        let stage_dir = tempfile::tempdir().unwrap();
        stage_the_fixture(stage_dir.path());
        commit_install(root, FIXTURE_ID, FIXTURE_VERSION, stage_dir.path()).unwrap();

        let plugin_data = data_root.join("plugin_data").join(FIXTURE_ID);
        std::fs::create_dir_all(&plugin_data).unwrap();
        std::fs::write(plugin_data.join("db"), b"state").unwrap();

        uninstall(root, FIXTURE_ID, true, data_root).unwrap();
        assert!(!root.join(FIXTURE_ID).exists());
        assert!(plugin_data.join("db").is_file(), "keep_data must preserve data");
    }

    // ── rollback ──────────────────────────────────────────────────────────

    #[test]
    fn rollback_repoints_current_to_prior_version() {
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();

        // Install v1 (the fixture version) then hand-make a v2 dir and point
        // current at it, then roll back to v1.
        let stage_dir = tempfile::tempdir().unwrap();
        stage_the_fixture(stage_dir.path());
        commit_install(root, FIXTURE_ID, FIXTURE_VERSION, stage_dir.path()).unwrap();

        let v2 = root.join(FIXTURE_ID).join("2.0.0");
        std::fs::create_dir_all(&v2).unwrap();
        std::fs::write(v2.join("manifest.json"), b"{}").unwrap();
        repoint_current(&root.join(FIXTURE_ID), "2.0.0").unwrap();
        assert_current_points_at(&root.join(FIXTURE_ID), "2.0.0");

        rollback(root, FIXTURE_ID, FIXTURE_VERSION).unwrap();
        assert_current_points_at(&root.join(FIXTURE_ID), FIXTURE_VERSION);
    }

    #[test]
    fn rollback_to_missing_version_errors() {
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path();
        std::fs::create_dir_all(root.join(FIXTURE_ID)).unwrap();
        let err = rollback(root, FIXTURE_ID, "9.9.9").unwrap_err();
        assert!(matches!(err, InstallError::Io(_)), "got {err:?}");
    }

    // ── InstallError Display ──────────────────────────────────────────────

    #[test]
    fn error_display_is_user_readable() {
        assert!(InstallError::Hash.to_string().contains("hash"));
        assert!(InstallError::Signature.to_string().contains("signature"));
        assert!(InstallError::IdMismatch.to_string().contains("id"));
        assert!(InstallError::Unpack("x".into()).to_string().contains("x"));
        assert!(InstallError::Manifest("y".into()).to_string().contains("y"));
        assert!(InstallError::Io("z".into()).to_string().contains("z"));
    }
}
