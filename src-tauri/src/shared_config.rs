use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SharedConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sotvault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rawvault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibre_path: Option<String>,
    /// Proxy for the `git` subprocess that drives vault sync, e.g.
    /// `http://127.0.0.1:1080` or `socks5://127.0.0.1:1080`.
    ///
    /// Machine-local on purpose. It lives here rather than in the vault's own
    /// `.notemd/settings.json` because that file syncs: a proxy that is correct
    /// on one machine is wrong (or a dead port) on every other one. Same
    /// reasoning as `sotvault` — see the module docs on what "shared" means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_proxy: Option<String>,
}

fn default_version() -> u32 { 1 }

/// 共享配置的位置:`<config_dir>/net.notemd.app/shared.json`。
///
/// 「共享」指的是**进程之间**(GUI / CLI / 插件二进制)共享,不是应用之间 ——
/// 所以它就住在 app 自己的配置目录里,和 `settings.json` 并排,不再单占一个顶层
/// 目录。`dirs::config_dir()` 在 macOS 上正是 `~/Library/Application Support`,
/// 与 Tauri 的 `app_config_dir()` 同址;Linux/Windows 也各自落到正确位置。
///
/// 插件二进制里有同名解析的副本(roam-import / claude-agent / ebook-import),
/// 改这里必须同步改那边并重发插件。
pub fn config_path() -> std::io::Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "config directory not found")
    })?;
    Ok(base.join(crate::app_dirs::BUNDLE_ID).join("shared.json"))
}

pub fn read(path: &Path) -> std::io::Result<SharedConfig> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or_else(|_| SharedConfig {
            version: 1, ..Default::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SharedConfig {
            version: 1, ..Default::default()
        }),
        Err(e) => Err(e),
    }
}

pub fn write(path: &Path, cfg: &SharedConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_missing_returns_default() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = read(&p).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.sotvault, None);
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = SharedConfig {
            version: 1,
            sotvault: Some("/tmp/sot".into()),
            rawvault: Some("/tmp/raw".into()),
            calibre_path: Some("/Applications/calibre.app/Contents/MacOS".into()),
            git_proxy: Some("http://127.0.0.1:1080".into()),
        };
        write(&p, &cfg).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back, cfg);
    }

    /// A config written before `git_proxy` existed must still load — the field
    /// is `#[serde(default)]`, and older builds / the plugin binaries that
    /// carry their own copy of this struct keep writing files without it.
    #[test]
    fn reads_a_config_written_before_git_proxy_existed() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, r#"{"version":1,"sotvault":"/v"}"#).unwrap();
        let cfg = read(&p).unwrap();
        assert_eq!(cfg.sotvault.as_deref(), Some("/v"));
        assert_eq!(cfg.git_proxy, None);
    }

    /// And an unset proxy must not appear in the file at all, so shared.json
    /// stays byte-stable for users who never touch the setting.
    #[test]
    fn an_unset_proxy_is_not_serialized() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        write(&p, &SharedConfig { version: 1, sotvault: Some("/v".into()), ..Default::default() }).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("git_proxy"), "{text}");
    }

    #[test]
    fn write_uses_atomic_tmp_rename() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = SharedConfig::default();
        write(&p, &cfg).unwrap();
        assert!(p.exists());
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupted_file_falls_back_to_default() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, "{ not valid json").unwrap();
        let cfg = read(&p).unwrap();
        assert_eq!(cfg.version, 1);
    }
}
