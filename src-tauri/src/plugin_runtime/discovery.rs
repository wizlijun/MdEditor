//! Discovery of installed v2 plugins: for every `enabled` entry in
//! state.json, load and validate `<root>/<id>/current/manifest.json`.
//! Any failure rejects that one plugin (with an eprintln) without
//! affecting the others (spec §3).

use plugin_protocol::ManifestV2;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::state;

/// Map the compile-time host arch to the manifest `binary` key (target triple).
/// Shared with commands.rs so SpawnCtx resolves the exact binary discovery
/// validated at scan time.
pub(crate) fn current_arch_triple() -> Option<&'static str> {
    arch_triple_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn arch_triple_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "x86") => Some("i686-pc-windows-msvc"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "x86") => Some("i686-unknown-linux-gnu"),
        _ => None,
    }
}

/// One state.json entry as seen by management-oriented callers.
///
/// Runtime discovery intentionally exposes only enabled, runnable plugins.
/// CLI management has a different job: it must also see disabled and broken
/// installations so users can enable, inspect, repair, update, or remove them.
#[derive(Debug)]
pub struct InstalledInventoryEntry {
    pub id: String,
    pub state_version: String,
    pub enabled: bool,
    pub current_dir: PathBuf,
    pub manifest: Result<ManifestV2, String>,
}

/// Enumerate every plugin recorded in state.json, retaining validation errors
/// instead of silently dropping the corresponding installation.
pub fn scan_root_inventory(root: &Path, host_version: &str) -> Vec<InstalledInventoryEntry> {
    state::load(root)
        .installed
        .into_iter()
        .map(|(id, entry)| {
            let current_dir = root.join(&id).join("current");
            let manifest = validate_installed(&current_dir, &id, host_version);
            InstalledInventoryEntry {
                id,
                state_version: entry.version,
                enabled: entry.enabled,
                current_dir,
                manifest,
            }
        })
        .collect()
}

/// Pure scan against an explicit plugins root (testable without an AppHandle).
/// Returns id → (manifest, absolute path of the plugin's `current/` dir).
pub fn scan_root(root: &Path, host_version: &str) -> BTreeMap<String, (ManifestV2, PathBuf)> {
    let mut out = BTreeMap::new();
    for entry in scan_root_inventory(root, host_version) {
        if !entry.enabled {
            continue;
        }
        match entry.manifest {
            Ok(m) => {
                out.insert(entry.id, (m, entry.current_dir));
            }
            Err(e) => eprintln!("[plugin_runtime] skipping plugin '{}': {e}", entry.id),
        }
    }
    out
}

/// Thin AppHandle wrapper: resolve the plugins root, then [`scan_root`].
pub fn scan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host_version: &str,
) -> Result<BTreeMap<String, (ManifestV2, PathBuf)>, String> {
    let root = state::plugins_root(app).ok_or("cannot resolve app data dir")?;
    Ok(scan_root(&root, host_version))
}

/// 校验一个已安装插件的 `current/` 目录:manifest 可读可解析、通过
/// `validate_manifest`、id 与安装目录一致、当前架构有二进制。
///
/// 公开是刻意的:`notemd doctor` 必须调用**这一个**实现来判断插件是否健康。
/// 若它在 doctor 里复刻一遍这串判断,两份必然漂移 —— 那正是这个项目反复踩过的
/// 「第二真相来源」坑。
pub fn validate_installed(
    current: &Path,
    dir_id: &str,
    host_version: &str,
) -> Result<ManifestV2, String> {
    let text = std::fs::read_to_string(current.join("manifest.json"))
        .map_err(|e| format!("manifest.json: {e}"))?;
    let m: ManifestV2 = serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    plugin_protocol::validate_manifest(&m, host_version)?;
    if m.id != dir_id {
        return Err(format!("manifest id '{}' != install dir '{dir_id}'", m.id));
    }
    // ②T1: `binary` is optional (ui-only plugins have none). Only a plugin that
    // declares a binary is subject to the arch/presence check; a ui-only
    // manifest (validated to have `ui`) skips it entirely. A plugin that DOES
    // declare binaries must still ship one for the host arch.
    if !m.binary.is_empty() {
        let triple = current_arch_triple()
            .ok_or_else(|| format!("unsupported host arch '{}'", std::env::consts::ARCH))?;
        let rel = m
            .binary
            .get(triple)
            .ok_or_else(|| format!("no binary for host arch '{triple}'"))?;
        let bin = current.join(rel);
        if !bin.is_file() {
            return Err(format!("binary '{}' does not exist", bin.display()));
        }
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_runtime::state::{InstallState, InstalledPlugin};
    use serde_json::json;

    const HOST: &str = "1.0.0";

    /// Minimal valid manifest in the md2pdf sample shape, keyed on the
    /// current arch triple (computed exactly like discovery does).
    fn md2pdf_manifest(engines: &str, binary_key: &str) -> serde_json::Value {
        json!({
            "manifest_version": 2,
            "id": "notemd.md2pdf",
            "name": "Export to PDF",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": engines },
            "binary": { binary_key: "bin/md2pdf-v2" },
            "activation": { "events": ["onCommand:export", "onCli:pdf2"] },
            "capabilities": ["toast"]
        })
    }

    fn write_state(root: &Path, entries: &[(&str, bool)]) {
        let mut s = InstallState::default();
        for (id, enabled) in entries {
            s.installed.insert(
                (*id).to_string(),
                InstalledPlugin {
                    version: "1.0.0".into(),
                    enabled: *enabled,
                },
            );
        }
        state::save(root, &s).unwrap();
    }

    /// Install a plugin under `<root>/<dir_id>/current/`: manifest.json plus
    /// (optionally) the dummy binary the manifest points at.
    fn install_plugin(root: &Path, dir_id: &str, manifest: &str, with_binary: bool) {
        let current = root.join(dir_id).join("current");
        std::fs::create_dir_all(current.join("bin")).unwrap();
        std::fs::write(current.join("manifest.json"), manifest).unwrap();
        if with_binary {
            std::fs::write(current.join("bin/md2pdf-v2"), b"#!/bin/sh\nexit 0\n").unwrap();
        }
    }

    fn triple() -> &'static str {
        current_arch_triple().expect("tests run on a supported arch")
    }

    #[test]
    fn happy_path_loads_one_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", true)]);
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);

        let map = scan_root(root, HOST);
        assert_eq!(map.len(), 1);
        let (m, current) = &map["notemd.md2pdf"];
        assert_eq!(m.id, "notemd.md2pdf");
        assert_eq!(*current, root.join("notemd.md2pdf").join("current"));
    }

    #[test]
    fn bad_json_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", true)]);
        install_plugin(root, "notemd.md2pdf", "{ not json", true);
        assert!(scan_root(root, HOST).is_empty());
    }

    #[test]
    fn unsatisfied_engines_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", true)]);
        let manifest = md2pdf_manifest(">=1.0.0", triple()).to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);
        // Host is older than what the plugin requires.
        assert!(scan_root(root, "0.0.1").is_empty());
    }

    #[test]
    fn missing_arch_binary_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", true)]);

        // Key for a different target: current arch has no binary entry.
        let manifest = md2pdf_manifest(">=0.0.0", "wasm32-unknown-unknown").to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);
        assert!(scan_root(root, HOST).is_empty());

        // Right key, but the file itself is missing.
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        std::fs::write(root.join("notemd.md2pdf/current/manifest.json"), &manifest).unwrap();
        std::fs::remove_file(root.join("notemd.md2pdf/current/bin/md2pdf-v2")).unwrap();
        assert!(scan_root(root, HOST).is_empty());
    }

    #[test]
    fn disabled_entry_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", false)]);
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);
        assert!(scan_root(root, HOST).is_empty());
    }

    #[test]
    fn inventory_retains_disabled_and_invalid_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", false), ("notemd.broken", true)]);
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);
        install_plugin(root, "notemd.broken", "{ not json", true);

        let inventory = scan_root_inventory(root, HOST);
        assert_eq!(inventory.len(), 2);
        let broken = inventory.iter().find(|e| e.id == "notemd.broken").unwrap();
        assert!(broken.enabled);
        assert!(broken.manifest.is_err());
        let disabled = inventory.iter().find(|e| e.id == "notemd.md2pdf").unwrap();
        assert!(!disabled.enabled);
        assert!(disabled.manifest.is_ok());
    }

    #[test]
    fn arch_triple_matches_the_current_os_and_arch() {
        let triple = current_arch_triple().expect("test host is supported");
        let expected_os_fragment = match std::env::consts::OS {
            "macos" => "apple-darwin",
            "windows" => "pc-windows-msvc",
            "linux" => "unknown-linux-gnu",
            other => panic!("unsupported test OS: {other}"),
        };
        assert!(
            triple.ends_with(expected_os_fragment),
            "unexpected triple: {triple}"
        );
    }

    #[test]
    fn arch_triple_maps_every_supported_desktop_target() {
        for (os, arch, expected) in [
            ("macos", "aarch64", "aarch64-apple-darwin"),
            ("macos", "x86_64", "x86_64-apple-darwin"),
            ("windows", "aarch64", "aarch64-pc-windows-msvc"),
            ("windows", "x86_64", "x86_64-pc-windows-msvc"),
            ("windows", "x86", "i686-pc-windows-msvc"),
            ("linux", "aarch64", "aarch64-unknown-linux-gnu"),
            ("linux", "x86_64", "x86_64-unknown-linux-gnu"),
            ("linux", "x86", "i686-unknown-linux-gnu"),
        ] {
            assert_eq!(arch_triple_for(os, arch), Some(expected));
        }
        assert_eq!(arch_triple_for("freebsd", "x86_64"), None);
        assert_eq!(arch_triple_for("windows", "mips"), None);
    }

    #[test]
    fn id_dir_mismatch_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Installed under a directory that doesn't match manifest.id.
        write_state(root, &[("notemd.other", true)]);
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        install_plugin(root, "notemd.other", &manifest, true);
        assert!(scan_root(root, HOST).is_empty());
    }

    #[test]
    fn one_bad_plugin_does_not_block_others() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_state(root, &[("notemd.md2pdf", true), ("notemd.broken", true)]);
        let manifest = md2pdf_manifest(">=0.0.0", triple()).to_string();
        install_plugin(root, "notemd.md2pdf", &manifest, true);
        install_plugin(root, "notemd.broken", "{ not json", true);

        let map = scan_root(root, HOST);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("notemd.md2pdf"));
    }
}
