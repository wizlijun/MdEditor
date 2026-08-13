//! Vault-scoped settings persisted to `{vault}/.notemd/settings.json` so they
//! travel with the git-synced vault. Holds the directory-name conventions
//! shared by sync-to-vault (`syncDir`) and outline notes (`wikipageDir`,
//! `dailynoteDir`). Missing fields fall back to per-field defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETTINGS_DIR: &str = ".notemd";
const SETTINGS_FILE: &str = "settings.json";

/// Default sync sub-directory when unset/invalid.
pub const DEFAULT_SYNC_DIR: &str = "sync";

/// Default quick-note inbox sub-directory when unset/invalid.
pub const DEFAULT_INBOX_DIR: &str = "inbox";

/// Default wikilink page sub-directory when unset/invalid — mirrors
/// `DEFAULT_DIRS.wikipage` in `src/lib/outline/dirs.svelte.ts`.
///
/// Lives here rather than beside either of its two consumers
/// (`plugin_runtime::ui_rpc`, which reports it to plugins, and
/// `search::options`, which pins the page named by a query) because a third
/// spelling of `"wikipage"` is exactly how the two would drift: the day
/// someone changes the shipped default, a copy left behind means the search
/// panel pins pages in a directory nothing else considers the wiki
/// directory.
pub const DEFAULT_WIKIPAGE_DIR: &str = "wikipage";

/// Raw parsed settings. Every field is optional: absent = "not configured",
/// so callers apply their own defaults (never persisted implicitly).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wikipage_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dailynote_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbox_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub large_file_threshold_mb: Option<u32>,
    /// Vault-relative directories excluded from the search index.
    ///
    /// Absent (not `[]`) means "never configured". Empty on purpose by default:
    /// which of your directories are not worth searching is your call, not ours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_exclude_dirs: Option<Vec<String>>,
    /// 索引跳过阈值,与 `large_file_threshold_mb`(git 大文件门禁)**语义不同**:
    /// 那个决定什么不进 commit,这个决定什么不进索引。原始资料类 md
    /// (ebook 导出、字幕转写)常态超过 1 MB,索引可以放宽而 git 门禁不该跟着放。
    ///
    /// 缺失时回落到 git 门禁的值(见 `search::options::for_vault`),所以
    /// 已经调过门禁的用户不会突然发现索引行为变了;一旦显式设过就彻底脱钩。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_large_file_threshold_mb: Option<u32>,
    /// 用户配置的原始资料 glob 模式(spec `.superpowers/sdd/2026-08-12-
    /// source-globs-and-transcript-indexing/`,§4.1)。语法本身极度宽容 ——
    /// `searchidx::globs::parse` 从不因为一条模式写不对就让整份列表失效,
    /// 所以这里不做语法校验,原样存、原样读,把容错留给 `parse` 自己。
    ///
    /// 缺省(`None`)与显式空列表(`Some(vec![])`)含义不同:`search::
    /// options::for_vault`(`ScanOptions.source_globs` 的唯一构造点)据此
    /// 决定要不要用当前解析出的 `syncDir` 种一条默认模式 —— 前者种,后者
    /// 不种,否则用户把列表清空后又会被悄悄种回去,永远清不掉。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_source_globs: Option<Vec<String>>,
    /// 按来源分层的排序权重覆盖(task C-T7 `searchidx::query::Weights` 的
    /// 四个字段)。每个子字段独立可选:缺一个字段只影响那一档的回落,不
    /// 牵连其余三档,是 `Weights::sanitized()`「坏值只回落那一档」这条
    /// 不变量在"没写"这种缺失形式上的自然延伸。
    ///
    /// **两道独立的校验,解决不同的问题(review round 1, Important 4;
    /// spec §8:「权重非法 → 保存时拒绝,保留原值」)。** `merge`(写入侧)
    /// 对每个*显式给出*的字段调用 `validate_search_weights`,越界值直接
    /// 拒绝整次保存 —— 否则设置页会把 `-1` 读回来当"当前值"渲染,而每次
    /// 查询其实仍在用 1.25,显示与生效永久不一致,且没有任何报错路径。
    /// `search::options::weights_for_vault`(权重的唯一构造点)在读取侧
    /// 仍然调用 `Weights::sanitized()` 兜底 —— 这道防线保护的是没经过
    /// `merge` 就进了 `settings.json` 的值(手改文件、未来某个不走这条
    /// 校验路径的写入方),让那种情况也总能拿到一个可用的结果,而不是让
    /// vault 打不开。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_weights: Option<SearchWeights>,
}

/// Plain data shape for [`VaultSettings::search_weights`] — see that field's
/// doc comment for the two-layer validation story (`merge` rejects at save
/// time, `Weights::sanitized()` defends at read time). Kept as four
/// independent `Option<f64>`s rather than `searchidx::query::Weights` itself
/// so this module, which otherwise has no dependency on the `searchidx`
/// crate's types, doesn't need one just to describe a settings shape.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWeights {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlabeled: Option<f64>,
}

fn settings_path(vault_root: &Path) -> PathBuf {
    vault_root.join(SETTINGS_DIR).join(SETTINGS_FILE)
}

/// Read settings; a missing file or malformed JSON both yield all-None
/// (this never errors — the vault should still open).
pub fn read(vault_root: &Path) -> VaultSettings {
    match std::fs::read_to_string(settings_path(vault_root)) {
        Ok(txt) => serde_json::from_str(&txt).unwrap_or_default(),
        Err(_) => VaultSettings::default(),
    }
}

/// Write settings, creating the `.notemd/` directory if needed.
pub fn write(vault_root: &Path, settings: &VaultSettings) -> Result<(), String> {
    let dir = vault_root.join(SETTINGS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let txt = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(vault_root), txt).map_err(|e| e.to_string())
}

/// Validate a vault-relative directory name. Rejects empty, absolute paths, and
/// any `..` segment; trims whitespace and collapses `.`/redundant separators.
///
/// "Absolute" includes Windows drive-relative and drive-absolute forms
/// (`C:\Users\…`, `C:notes`), not just a leading separator. Every value this
/// guards — `sync_dir`, `wikipage_dir`, `dailynote_dir`, `inbox_dir` — is fed
/// to `Path::join` against the vault root, and on Windows joining a path with
/// a drive letter *replaces* the base entirely rather than appending to it.
/// A setting that reads "a directory inside the vault" would silently become
/// an arbitrary location on disk. The check is on any segment, not just the
/// first, because the segment loop below drops `.` segments — so `./C:\x`
/// would otherwise arrive at `join` as `C:\x`.
pub fn validate_rel_dir(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("directory name is empty".into());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err("directory must be relative".into());
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split(['/', '\\']) {
        let seg = seg.trim();
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err("directory must stay within the vault".into());
        }
        if has_drive_prefix(seg) {
            return Err("directory must be relative".into());
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        return Err("directory name is empty".into());
    }
    Ok(parts.join("/"))
}

/// `^[A-Za-z]:` — a Windows drive designator. Checked on every target, not
/// only `cfg(windows)`: settings live in `.notemd/settings.json` inside the
/// vault, which syncs between machines, so a value written on macOS must not
/// become a path-traversal on the Windows box that reads it next.
fn has_drive_prefix(seg: &str) -> bool {
    let mut c = seg.chars();
    matches!((c.next(), c.next()), (Some(a), Some(':')) if a.is_ascii_alphabetic())
}

/// Merge caller-provided (Some) fields onto `base`, validating each provided
/// value. Fields left None keep their base value. Returns the first validation
/// error encountered.
///
/// `search_source_globs` is stored verbatim, with no validation — see that
/// field's doc comment on [`VaultSettings`] for why (`searchidx::globs::
/// parse` is already tolerant downstream). `search_weights` **is**
/// validated here — each explicitly-provided component must be finite,
/// > 0.0, and <= 5.0 (`validate_search_weights`, [`SearchWeights`]'s doc
/// comment has the fuller rationale) — a save-time rejection on top of,
/// not instead of, `Weights::sanitized()`'s read-time fallback.
#[allow(clippy::too_many_arguments)]
pub fn merge(
    base: VaultSettings,
    sync_dir: Option<String>,
    wikipage_dir: Option<String>,
    dailynote_dir: Option<String>,
    large_file_threshold_mb: Option<u32>,
    inbox_dir: Option<String>,
    search_exclude_dirs: Option<Vec<String>>,
    search_large_file_threshold_mb: Option<u32>,
    search_source_globs: Option<Vec<String>>,
    search_weights: Option<SearchWeights>,
) -> Result<VaultSettings, String> {
    let mut out = base;
    if let Some(v) = sync_dir {
        out.sync_dir = Some(validate_rel_dir(&v)?);
    }
    if let Some(v) = wikipage_dir {
        out.wikipage_dir = Some(validate_rel_dir(&v)?);
    }
    if let Some(v) = dailynote_dir {
        out.dailynote_dir = Some(validate_rel_dir(&v)?);
    }
    if let Some(v) = inbox_dir {
        out.inbox_dir = Some(validate_rel_dir(&v)?);
    }
    if let Some(mb) = large_file_threshold_mb {
        if mb == 0 {
            return Err("large file threshold must be at least 1 MB".into());
        }
        out.large_file_threshold_mb = Some(mb);
    }
    if let Some(list) = search_exclude_dirs {
        let mut out_dirs = Vec::with_capacity(list.len());
        for raw in list {
            out_dirs.push(validate_rel_dir(&raw)?);
        }
        out.search_exclude_dirs = Some(out_dirs);
    }
    if let Some(mb) = search_large_file_threshold_mb {
        if mb == 0 {
            return Err("search index threshold must be at least 1 MB".into());
        }
        out.search_large_file_threshold_mb = Some(mb);
    }
    if let Some(list) = search_source_globs {
        out.search_source_globs = Some(list);
    }
    if let Some(w) = search_weights {
        validate_search_weights(&w)?;
        out.search_weights = Some(w);
    }
    Ok(out)
}

/// Reject an out-of-range weight at save time (spec §8: "权重非法 → 保存时
/// 拒绝,保留原值"). Bounds match `searchidx::query::Weights::sanitized`'s
/// rule exactly (finite, > 0.0, <= 5.0) so a value that would have been
/// silently corrected on read is instead refused before it ever reaches
/// disk — see [`VaultSettings::search_weights`]'s doc comment for why both
/// checks exist rather than just one. Only *provided* components are
/// checked: an absent field is "not configured," not "invalid," and merging
/// a partial `SearchWeights` must not fail just because the caller didn't
/// mention `unlabeled`.
pub(crate) fn validate_search_weights(w: &SearchWeights) -> Result<(), String> {
    fn check(name: &str, v: Option<f64>) -> Result<(), String> {
        match v {
            Some(x) if !x.is_finite() || x <= 0.0 || x > 5.0 => {
                Err(format!("{name} weight must be a finite number greater than 0 and at most 5.0"))
            }
            _ => Ok(()),
        }
    }
    check("human", w.human)?;
    check("derived", w.derived)?;
    check("source", w.source)?;
    check("unlabeled", w.unlabeled)?;
    Ok(())
}

/// The effective sync sub-directory: the configured value when present and
/// valid, otherwise [`DEFAULT_SYNC_DIR`]. Reads `.notemd/settings.json` itself
/// — for a caller that already has a `VaultSettings` in hand, use
/// [`resolve_sync_dir_from`] instead so the file is only read once.
///
/// **As of C-T6, this value no longer reaches the search index at all.**
/// Historically it did: `origin::derive`'s old rule 5 read `sync_dir`
/// directly, and `store::open`'s staleness stamp existed to invalidate every
/// stored `origin` when it changed — `search::mod::open_vault` and
/// `cli::search::run`, the two `SearchIndex::open` call sites, each resolved
/// `sync_dir` themselves (a second, independent `.notemd/settings.json` read
/// alongside the one already done for `search::options::for_vault`'s other
/// fields — see the C-T3 task report for why that duplication was an
/// accepted, narrow trade) and passed it straight into `SearchIndex::open`.
/// Rule 5 was retired in favor of user-configured source-glob patterns (rule
/// 5′) back in C-T2, and C-T6 repointed `store::open`'s staleness stamp at
/// `SourceGlobs::stamp()` to match — both of those two call sites now compute
/// a glob stamp instead, off `search::options::for_vault`'s real
/// `search_source_globs`-backed value (wired by C-T8; see the call sites
/// themselves). This function is unchanged and still resolves the *sync mirror* directory
/// name correctly for its remaining callers (`sotvault::mod` for the actual
/// sync-to-vault mirror, `plugin_runtime::ui_rpc` for the plugin RPC surface)
/// — it just no longer has anything to do with the search index.
pub fn resolve_sync_dir(vault_root: &Path) -> String {
    resolve_sync_dir_from(&read(vault_root))
}

/// Same effective value as [`resolve_sync_dir`], computed from an
/// already-read `VaultSettings` rather than reading the file itself.
pub fn resolve_sync_dir_from(vs: &VaultSettings) -> String {
    vs.sync_dir
        .as_deref()
        .and_then(|v| validate_rel_dir(v).ok())
        .unwrap_or_else(|| DEFAULT_SYNC_DIR.to_string())
}

/// The effective quick-note inbox sub-directory: the configured value when
/// present and valid, otherwise [`DEFAULT_INBOX_DIR`].
pub fn resolve_inbox_dir(vault_root: &Path) -> String {
    read(vault_root)
        .inbox_dir
        .and_then(|v| validate_rel_dir(&v).ok())
        .unwrap_or_else(|| DEFAULT_INBOX_DIR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_missing_file_is_all_none() {
        let dir = TempDir::new().unwrap();
        assert_eq!(read(dir.path()), VaultSettings::default());
    }

    #[test]
    fn read_malformed_json_is_all_none() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(SETTINGS_DIR)).unwrap();
        std::fs::write(settings_path(dir.path()), "{ not json").unwrap();
        assert_eq!(read(dir.path()), VaultSettings::default());
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = TempDir::new().unwrap();
        let s = VaultSettings {
            sync_dir: Some("sync".into()),
            wikipage_dir: Some("wiki".into()),
            dailynote_dir: None,
            inbox_dir: Some("inbox".into()),
            large_file_threshold_mb: None,
            search_exclude_dirs: None,
            search_large_file_threshold_mb: None,
            search_source_globs: None,
            search_weights: None,
        };
        write(dir.path(), &s).unwrap();
        assert_eq!(read(dir.path()), s);
    }

    #[test]
    fn write_creates_notemd_dir() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), &VaultSettings::default()).unwrap();
        assert!(dir.path().join(SETTINGS_DIR).join(SETTINGS_FILE).is_file());
    }

    #[test]
    fn validate_accepts_relative_and_nested() {
        assert_eq!(validate_rel_dir("sync").unwrap(), "sync");
        assert_eq!(validate_rel_dir("  Sync  ").unwrap(), "Sync");
        assert_eq!(validate_rel_dir("Attachments/sync").unwrap(), "Attachments/sync");
        assert_eq!(validate_rel_dir("a//b/").unwrap(), "a/b");
    }

    #[test]
    fn validate_rejects_empty_absolute_and_dotdot() {
        assert!(validate_rel_dir("").is_err());
        assert!(validate_rel_dir("   ").is_err());
        assert!(validate_rel_dir("/abs").is_err());
        assert!(validate_rel_dir("\\abs").is_err());
        assert!(validate_rel_dir("../escape").is_err());
        assert!(validate_rel_dir("a/../b").is_err());
    }

    /// On Windows, `Path::join` with a drive-letter argument REPLACES the base
    /// instead of appending to it — so `vault_root.join("C:\\Users\\x")` is
    /// `C:\Users\x`, and a setting that reads "a directory inside the vault"
    /// silently becomes an arbitrary location on disk. `sync_dir`,
    /// `wikipage_dir`, `dailynote_dir` and `inbox_dir` all flow into `join`.
    /// Rejected on every platform, not just `cfg(windows)`: settings live in
    /// the vault and travel between machines.
    #[test]
    fn validate_rejects_windows_drive_letters() {
        assert!(validate_rel_dir("C:\\Users\\x").is_err());
        assert!(validate_rel_dir("c:/Users/x").is_err());
        // Drive-*relative* ("the current directory on drive C"), which has no
        // leading separator to catch it.
        assert!(validate_rel_dir("C:notes").is_err());
        // `.` segments are dropped by the loop, so the drive must be rejected
        // wherever it appears, not only at position zero.
        assert!(validate_rel_dir("./C:\\Users\\x").is_err());
        assert!(validate_rel_dir("notes/D:\\elsewhere").is_err());
        // A colon that is not a drive designator is still an ordinary (if
        // unusual) directory name on unix and must keep working.
        assert_eq!(validate_rel_dir("notes:archive").unwrap(), "notes:archive");
    }

    #[test]
    fn merge_keeps_untouched_fields() {
        let base = VaultSettings {
            sync_dir: Some("sync".into()),
            wikipage_dir: Some("wiki".into()),
            dailynote_dir: Some("daily".into()),
            inbox_dir: Some("inbox".into()),
            large_file_threshold_mb: None,
            search_exclude_dirs: None,
            search_large_file_threshold_mb: None,
            search_source_globs: None,
            search_weights: None,
        };
        let out = merge(base, Some("box".into()), None, None, None, None, None, None, None, None).unwrap();
        assert_eq!(out.sync_dir.as_deref(), Some("box"));
        assert_eq!(out.wikipage_dir.as_deref(), Some("wiki"));
        assert_eq!(out.dailynote_dir.as_deref(), Some("daily"));
        assert_eq!(out.inbox_dir.as_deref(), Some("inbox"));
    }

    #[test]
    fn merge_rejects_invalid_provided_value() {
        assert!(merge(VaultSettings::default(), Some("../x".into()), None, None, None, None, None, None, None, None).is_err());
        assert!(merge(VaultSettings::default(), None, None, None, None, Some("../x".into()), None, None, None, None).is_err());
    }

    #[test]
    fn merge_sets_inbox_dir() {
        let out = merge(VaultSettings::default(), None, None, None, None, Some("box".into()), None, None, None, None).unwrap();
        assert_eq!(out.inbox_dir.as_deref(), Some("box"));
    }

    #[test]
    fn resolve_inbox_dir_defaults_and_uses_configured() {
        let dir = TempDir::new().unwrap();
        assert_eq!(resolve_inbox_dir(dir.path()), DEFAULT_INBOX_DIR);
        write(
            dir.path(),
            &VaultSettings { inbox_dir: Some("box".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(resolve_inbox_dir(dir.path()), "box");
    }

    #[test]
    fn resolve_sync_dir_defaults_when_unset() {
        let dir = TempDir::new().unwrap();
        assert_eq!(resolve_sync_dir(dir.path()), DEFAULT_SYNC_DIR);
    }

    #[test]
    fn resolve_sync_dir_uses_configured_value() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            &VaultSettings { sync_dir: Some("box".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(resolve_sync_dir(dir.path()), "box");
    }

    #[test]
    fn resolve_sync_dir_falls_back_when_configured_value_invalid() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            &VaultSettings { sync_dir: Some("../nope".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(resolve_sync_dir(dir.path()), DEFAULT_SYNC_DIR);
    }

    /// review round 1, Minor #3: `resolve_sync_dir_from` must agree with
    /// `resolve_sync_dir` on every case above (default/configured/invalid) —
    /// it is the single-read variant `search::options::for_vault` uses so it
    /// does not read `.notemd/settings.json` twice per call.
    #[test]
    fn resolve_sync_dir_from_agrees_with_resolve_sync_dir_on_every_case() {
        assert_eq!(resolve_sync_dir_from(&VaultSettings::default()), DEFAULT_SYNC_DIR);
        assert_eq!(
            resolve_sync_dir_from(&VaultSettings { sync_dir: Some("box".into()), ..Default::default() }),
            "box"
        );
        assert_eq!(
            resolve_sync_dir_from(&VaultSettings { sync_dir: Some("../nope".into()), ..Default::default() }),
            DEFAULT_SYNC_DIR
        );
    }

    #[test]
    fn merge_sets_and_validates_threshold() {
        let out = merge(VaultSettings::default(), None, None, None, Some(25), None, None, None, None, None).unwrap();
        assert_eq!(out.large_file_threshold_mb, Some(25));
        assert!(merge(VaultSettings::default(), None, None, None, Some(0), None, None, None, None, None).is_err());
    }

    #[test]
    fn merge_keeps_threshold_when_none() {
        let base = VaultSettings { large_file_threshold_mb: Some(50), ..Default::default() };
        let out = merge(base, Some("box".into()), None, None, None, None, None, None, None, None).unwrap();
        assert_eq!(out.large_file_threshold_mb, Some(50));
    }

    #[test]
    fn threshold_round_trips() {
        let dir = TempDir::new().unwrap();
        let s = VaultSettings { large_file_threshold_mb: Some(10), ..Default::default() };
        write(dir.path(), &s).unwrap();
        assert_eq!(read(dir.path()), s);
    }

    #[test]
    fn search_exclude_dirs_round_trips_and_defaults_to_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = VaultSettings { search_exclude_dirs: Some(vec!["sessions".into()]), ..Default::default() };
        write(tmp.path(), &s).unwrap();
        assert_eq!(read(tmp.path()).search_exclude_dirs, Some(vec!["sessions".to_string()]));

        // 未设置时不写进文件:没碰过这个设置的用户,settings.json 必须逐字节不变。
        write(tmp.path(), &VaultSettings::default()).unwrap();
        let txt = std::fs::read_to_string(tmp.path().join(".notemd/settings.json")).unwrap();
        assert!(!txt.contains("searchExcludeDirs"), "{txt}");
    }

    /// 每一项都过 validate_rel_dir:绝对路径与 `..` 不能变成扫描排除规则。
    #[test]
    fn merge_validates_every_exclude_entry() {
        let ok = merge(
            VaultSettings::default(),
            None, None, None, None, None,
            Some(vec!["a/b".into()]),
            None, None, None,
        )
        .unwrap();
        assert_eq!(ok.search_exclude_dirs, Some(vec!["a/b".to_string()]));
        assert!(merge(
            VaultSettings::default(),
            None, None, None, None, None,
            Some(vec!["../x".into()]),
            None, None, None,
        )
        .is_err());
        assert!(merge(
            VaultSettings::default(),
            None, None, None, None, None,
            Some(vec!["/abs".into()]),
            None, None, None,
        )
        .is_err());
        assert!(merge(
            VaultSettings::default(),
            None, None, None, None, None,
            Some(vec!["C:\\x".into()]),
            None, None, None,
        )
        .is_err());
    }

    /// The drive-letter rejection has to hold on the fields that actually feed
    /// `Path::join`, not only on the (inert) exclude list — `merge` is the one
    /// door all of them come through.
    #[test]
    fn merge_rejects_a_drive_letter_in_every_directory_field() {
        let drive = || Some("C:\\Users\\x".to_string());
        let base = VaultSettings::default;
        assert!(merge(base(), drive(), None, None, None, None, None, None, None, None).is_err(), "sync_dir");
        assert!(merge(base(), None, drive(), None, None, None, None, None, None, None).is_err(), "wikipage_dir");
        assert!(merge(base(), None, None, drive(), None, None, None, None, None, None).is_err(), "dailynote_dir");
        assert!(merge(base(), None, None, None, None, drive(), None, None, None, None).is_err(), "inbox_dir");
    }

    /// 空数组是有意义的输入(= 清空排除),不能被当成"没提供"。
    #[test]
    fn an_empty_list_clears_the_exclusions() {
        let base = VaultSettings { search_exclude_dirs: Some(vec!["x".into()]), ..Default::default() };
        let out = merge(base, None, None, None, None, None, Some(vec![]), None, None, None).unwrap();
        assert_eq!(out.search_exclude_dirs, Some(vec![]));
    }

    #[test]
    fn search_threshold_round_trips_and_is_absent_when_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let s = VaultSettings { search_large_file_threshold_mb: Some(50), ..Default::default() };
        write(tmp.path(), &s).unwrap();
        assert_eq!(read(tmp.path()).search_large_file_threshold_mb, Some(50));

        write(tmp.path(), &VaultSettings::default()).unwrap();
        let txt = std::fs::read_to_string(tmp.path().join(".notemd/settings.json")).unwrap();
        assert!(!txt.contains("searchLargeFileThresholdMb"), "{txt}");
    }

    #[test]
    fn merge_rejects_a_zero_search_threshold() {
        let base = VaultSettings::default();
        assert!(merge(base, None, None, None, None, None, None, Some(0), None, None).is_err());
    }

    /// Review round 1, Important 4 (spec §8: "权重非法 → 保存时拒绝,保留
    /// 原值"). Each of `Weights::sanitized`'s rejected shapes — zero,
    /// negative, above 5.0, non-finite — must be refused at save time, not
    /// silently stored and only caught later by the read-side fallback.
    #[test]
    fn merge_rejects_an_out_of_range_weight() {
        let bad = |w: SearchWeights| {
            merge(VaultSettings::default(), None, None, None, None, None, None, None, None, Some(w))
        };
        assert!(bad(SearchWeights { human: Some(0.0), ..Default::default() }).is_err(), "0 不合法");
        assert!(bad(SearchWeights { human: Some(-1.0), ..Default::default() }).is_err(), "负数不合法");
        assert!(bad(SearchWeights { source: Some(5.000_001), ..Default::default() }).is_err(), "超过 5.0 不合法");
        assert!(bad(SearchWeights { derived: Some(f64::NAN), ..Default::default() }).is_err(), "NaN 不合法");
        assert!(bad(SearchWeights { unlabeled: Some(f64::INFINITY), ..Default::default() }).is_err(), "无穷不合法");
    }

    #[test]
    fn merge_accepts_boundary_and_ordinary_weights() {
        let ok = |w: SearchWeights| {
            merge(VaultSettings::default(), None, None, None, None, None, None, None, None, Some(w))
        };
        assert!(ok(SearchWeights { human: Some(5.0), ..Default::default() }).is_ok(), "5.0 是合法上界");
        assert!(ok(SearchWeights { source: Some(0.000_1), ..Default::default() }).is_ok(), "接近 0 但仍为正的合法值");
        assert!(ok(SearchWeights::default()).is_ok(), "全部缺省仍合法");
    }

    /// 缺省字段不是非法值 —— 只提供部分字段的更新不该因为没提到的那几档
    /// 而失败,`validate_search_weights` 只检查显式给出的字段。
    #[test]
    fn merge_does_not_reject_an_absent_component() {
        let out = merge(
            VaultSettings::default(),
            None, None, None, None, None, None, None, None,
            Some(SearchWeights { human: Some(2.0), ..Default::default() }),
        )
        .unwrap();
        assert_eq!(out.search_weights.unwrap().human, Some(2.0));
    }

    /// "保存时拒绝,保留原值": a rejected merge must not mutate `base` — the
    /// caller (`sotvault::persist_vault_settings`) never reaches its own
    /// `write` call when `merge` returns `Err`, so this pins the half of
    /// that guarantee that lives in this function: the `Err` path returns
    /// before touching `out.search_weights` at all.
    #[test]
    fn a_rejected_weight_does_not_disturb_the_existing_value() {
        let base = VaultSettings {
            search_weights: Some(SearchWeights { human: Some(1.5), ..Default::default() }),
            ..Default::default()
        };
        let err = merge(
            base,
            None, None, None, None, None, None, None, None,
            Some(SearchWeights { human: Some(-1.0), ..Default::default() }),
        );
        assert!(err.is_err(), "非法权重必须拒绝整次保存");
    }

    // `sync_dir_changed` (and its test, formerly here) was retired by C-T8.
    // It gated `notemd_vault_settings_set`'s index reopen on `syncDir`
    // changes back when `origin::derive`'s rule 5 read `sync_dir` directly.
    // Rule 5 was retired in favor of source globs (rule 5′) back in C-T2,
    // and C-T6 repointed the index's staleness stamp at `SourceGlobs::
    // stamp()` — a value `sync_dir` no longer feeds at all (see
    // `resolve_sync_dir`'s doc comment above for the fuller history) — which
    // left `sync_dir_changed` gating a reopen that recomputed an identical
    // stamp and rebuilt nothing: harmless, but pointless. Its own doc
    // comment said as much and explicitly deferred deletion to "whenever a
    // `search_source_globs_changed` sibling is added" rather than as a
    // drive-by — that moment is this task. See `search::options::
    // search_source_globs_changed`, which now does this gating job for
    // real: `search_source_globs` (unlike `sync_dir`) *is* the value the
    // stamp is a function of, so it is the one worth reopening the index
    // over.
}
