//! `ScanOptions` 的**唯一**构造点,`Weights` 的**唯一**构造点。
//!
//! GUI 与 CLI 是两个进程,写同一个索引库、读同一份排序权重。它们的扫描口径
//! 和排序口径都必须逐字段一致 —— 否则同一个 vault 会被两套阈值/排除规则
//! 索引,或者同一次搜索在两个入口给出不同的名次,而这个功能的全部前提是
//! 「一个算法,三个 adapter」。所以这里各自只有一个函数,两边都调它;
//! `tests/search_scan_options_contract.rs` 钉住这一点。

use std::path::Path;

use searchidx::globs::SourceGlobs;
use searchidx::query::{Conventions, Weights};
use searchidx::ScanOptions;

use crate::sotvault::vault_settings::{self, VaultSettings};

/// 未配置任何阈值时的默认值。与 git 大文件门禁的默认值相同,但这是巧合
/// 不是耦合 —— 两者语义不同,可以各自演化。
const DEFAULT_THRESHOLD_MB: u32 = 10;

/// `.notemd/settings.json` 只读一次 —— 两次独立读取之间可能被一次写入
/// 插队,产出撕裂的配置(`vault_settings` 模块自己的文档就是这条教训)。
/// `for_vault`/`weights_for_vault` 各自内部只调一次 `vault_settings::read`;
/// `source_globs_from`/`weights_from` 都从已经读好的 `&VaultSettings` 取值,
/// 而不是各自再读一遍文件,是同一条"只读一次"纪律在多字段场景下的延伸。
pub fn for_vault(vault_root: &Path) -> ScanOptions {
    let vs = vault_settings::read(vault_root);
    ScanOptions {
        // 回落链:索引阈值 → git 门禁 → 默认。中间那一跳是一次性的善意,
        // 让既有用户的索引行为不因为这次拆分而改变。
        large_file_threshold_mb: vs
            .search_large_file_threshold_mb
            .or(vs.large_file_threshold_mb)
            .unwrap_or(DEFAULT_THRESHOLD_MB),
        exclude_dirs: vs.search_exclude_dirs.clone().unwrap_or_default(),
        source_globs: source_globs_from(&vs),
    }
}

/// The `ScanOptions.source_globs` half of `for_vault`, factored out so both
/// this module's `search_source_globs_changed` and `for_vault` itself share
/// one seeding rule rather than two copies drifting apart.
///
/// Absent (`None`) vs. explicitly empty (`Some(vec![])`) are different
/// answers, not two spellings of the same thing:
/// - Absent means "never configured" — the state on upgrade, before the
///   settings page (C-T11) exists at all. Seeded with `<syncDir>/**`, using
///   the vault's **currently resolved** sync directory (not the literal
///   `"sync/**"`) so a user who renamed their mirror directory does not find
///   their mirrored files silently reclassified as `Origin::Unlabeled`
///   instead of `Origin::Source` the moment this task ships.
/// - Explicitly empty means the user cleared the list on purpose. Re-seeding
///   it here would make that impossible to express — every read would
///   silently put the default pattern back.
pub(crate) fn source_globs_from(vs: &VaultSettings) -> SourceGlobs {
    match &vs.search_source_globs {
        Some(patterns) => searchidx::globs::parse(patterns),
        None => {
            let sync_dir = vault_settings::resolve_sync_dir_from(vs);
            searchidx::globs::parse(&[format!("{sync_dir}/**")])
        }
    }
}

/// Did a settings write change the **effective** source-glob patterns —
/// compared on the resolved `SourceGlobs::stamp()`, not the raw JSON field,
/// so a save that reorders/dedupes/whitespace-tweaks the list (all of which
/// `SourceGlobs::stamp()` already treats as identical, see that method's
/// doc comment) does not trigger a rebuild for a no-op edit.
///
/// This is the gate `notemd_vault_settings_set` now reopens the index on,
/// replacing the retired `vault_settings::sync_dir_changed` — see that
/// function's former doc comment (still readable in git history / the
/// task-8 report) for why `sync_dir` stopped being the right thing to gate
/// on back in C-T6, and this crate's own doc comment above `for_vault` for
/// why `source_globs` *is* the right thing: it is the field `ScanOptions`
/// actually carries and `store::open`'s staleness stamp is a function of,
/// so a change here is the one that can leave stored rows (and their
/// `origin`) wrong until the index is reopened.
pub fn search_source_globs_changed(before: &VaultSettings, after: &VaultSettings) -> bool {
    source_globs_from(before).stamp() != source_globs_from(after).stamp()
}

/// The single construction point for [`Weights`] — the GUI (`search::mod::
/// search_locked`, behind `notemd_search`) and the CLI (`cli::search::run`,
/// via the `weights_for` delegate) both resolve a vault's ranking weights
/// through this function and both actually rank a query with the result
/// (`SearchIndex::search_with_weights`), never by reading `search_weights`
/// themselves, so the two adapters cannot silently rank the same vault
/// differently — and neither can silently ignore a configured value while
/// only *resolving* it correctly (review round 1, Important 2: that gap
/// existed for one round with no test able to catch it). Mirrors
/// `for_vault`'s shape and its "one read" discipline.
pub fn weights_for_vault(vault_root: &Path) -> Weights {
    weights_from(&vault_settings::read(vault_root))
}

/// The `&VaultSettings`-in-hand variant of [`weights_for_vault`], the same
/// `resolve_*_from` idiom `vault_settings::resolve_sync_dir_from` already
/// established — `pub(crate)`, like [`source_globs_from`]'s sibling, so a
/// future caller elsewhere in this crate that needs both `ScanOptions` and
/// `Weights` for the same vault can build both off one `vault_settings::
/// read` call instead of paying for the file twice (review round 1, Minor
/// 6: an earlier version of this doc comment claimed that was already
/// possible while both helpers were private, which no outside caller could
/// actually act on).
///
/// Every component falls back to [`Weights::default`]'s shipped constant
/// independently: a missing `search_weights` key, a missing individual
/// field within it, and an explicit out-of-range value (0, negative,
/// non-finite, or above 5.0 — [`Weights::sanitized`]'s rule) all land on
/// that field's default without disturbing the other three siblings. This
/// is the first production call site for `Weights::sanitized()` — C-T7 built
/// it but had no real caller yet (see that task's report).
pub(crate) fn weights_from(vs: &VaultSettings) -> Weights {
    let default = Weights::default();
    let sw = vs.search_weights.unwrap_or_default();
    Weights {
        human: sw.human.unwrap_or(default.human),
        derived: sw.derived.unwrap_or(default.derived),
        source: sw.source.unwrap_or(default.source),
        unlabeled: sw.unlabeled.unwrap_or(default.unlabeled),
        attention: sw.attention.unwrap_or(default.attention),
    }
    .sanitized()
}

/// The single construction point for [`Conventions`] — the third one in this
/// module, and it exists for the same reason as the other two: `wikipageDir`
/// decides which note gets pinned to the top of a result list, so a GUI and a
/// CLI that resolved it separately would answer the same query with different
/// first results. `tests/search_scan_options_contract.rs` pins that both go
/// through here.
///
/// An unset (or invalid) value falls back to the shipped default rather than
/// to "no pinning": almost no vault has ever touched this setting, and those
/// vaults are exactly the ones the feature has to work for out of the box.
/// Validation is `validate_rel_dir`, the same gate `plugin_runtime::ui_rpc`
/// applies to this same field — a configured `../escape` must not become a
/// path prefix that ranking then compares against.
pub fn conventions_for_vault(vault_root: &Path) -> Conventions {
    conventions_from(&vault_settings::read(vault_root))
}

/// The `&VaultSettings`-in-hand variant of [`conventions_for_vault`], same
/// idiom (and same "one read" discipline) as [`weights_from`].
pub(crate) fn conventions_from(vs: &VaultSettings) -> Conventions {
    let dir = vs
        .wikipage_dir
        .as_deref()
        .and_then(|s| vault_settings::validate_rel_dir(s).ok())
        .unwrap_or_else(|| vault_settings::DEFAULT_WIKIPAGE_DIR.to_string());
    Conventions { wikipage_dir: Some(dir) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sotvault::vault_settings::SearchWeights;

    fn write_settings(root: &Path, json: &str) {
        std::fs::create_dir_all(root.join(".notemd")).unwrap();
        std::fs::write(root.join(".notemd/settings.json"), json).unwrap();
    }

    /// 首次升级时模式为空,用**当前解析出的** syncDir 种一条,而不是字面量
    /// "sync/**" —— 改过镜像目录名的用户否则会突然发现镜像文件不算原始
    /// 资料。
    #[test]
    fn an_absent_glob_list_is_seeded_from_the_resolved_sync_dir() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"syncDir": "box"}"#);
        let opts = for_vault(d.path());
        assert!(opts.source_globs.matches("box/x.md"), "必须匹配已解析出的 syncDir");
        assert!(!opts.source_globs.matches("sync/x.md"), "不得回落到字面量 sync/**");
    }

    #[test]
    fn an_explicit_empty_list_is_respected_not_reseeded() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"searchSourceGlobs": []}"#);
        let opts = for_vault(d.path());
        assert!(opts.source_globs.is_empty(), "显式清空后不得再种回 syncDir");
        assert!(!opts.source_globs.matches("sync/x.md"));
    }

    /// 未设过 syncDir 时,种子模式落回默认 "sync/**"。
    #[test]
    fn an_absent_glob_list_with_no_sync_dir_seeds_the_default() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{}"#);
        let opts = for_vault(d.path());
        assert!(opts.source_globs.matches("sync/x.md"));
        assert!(!opts.source_globs.matches("box/x.md"));
    }

    /// 一份显式配置的模式列表按字面使用,不叠加 syncDir 种子。
    #[test]
    fn an_explicit_glob_list_is_used_verbatim() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"syncDir": "box", "searchSourceGlobs": ["ebook/**"]}"#);
        let opts = for_vault(d.path());
        assert!(opts.source_globs.matches("ebook/a.md"));
        assert!(!opts.source_globs.matches("box/a.md"), "显式列表不应再隐式包含 syncDir");
    }

    #[test]
    fn weights_fall_back_to_defaults_when_unset_or_invalid() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{}"#);
        assert_eq!(weights_for_vault(d.path()), Weights::default(), "缺字段 → 全部回落默认");

        write_settings(d.path(), r#"{"searchWeights": {"human": 0}}"#);
        let w = weights_for_vault(d.path());
        let default = Weights::default();
        assert_eq!(w.human, default.human, "0 是非法值,该档回落默认");
        assert_eq!(w.derived, default.derived, "未触碰的其余三档不受影响");
        assert_eq!(w.source, default.source);
        assert_eq!(w.unlabeled, default.unlabeled);

        write_settings(d.path(), r#"{"searchWeights": {"human": 2.0}}"#);
        let w = weights_for_vault(d.path());
        assert_eq!(w.human, 2.0, "合法的显式值必须生效");
        assert_eq!(w.derived, default.derived, "缺失的字段各自独立回落");
    }

    /// `search_source_globs_changed` 判的是解析出的 stamp,不是原始字段 ——
    /// 顺序/空白/首尾斜杠不同但语义相同的两份列表不该触发重建。
    #[test]
    fn search_source_globs_changed_compares_the_resolved_stamp() {
        let with = |patterns: Option<Vec<&str>>| VaultSettings {
            search_source_globs: patterns.map(|v| v.into_iter().map(String::from).collect()),
            ..Default::default()
        };
        assert!(
            search_source_globs_changed(&with(Some(vec!["a/**"])), &with(Some(vec!["b/**"]))),
            "真正的模式变化必须触发重开"
        );
        assert!(
            search_source_globs_changed(&with(None), &with(Some(vec![]))),
            "缺省(种 syncDir)到显式清空是真变化"
        );
        assert!(
            !search_source_globs_changed(
                &with(Some(vec!["a/**", "b/**"])),
                &with(Some(vec!["b/**", "a/**"]))
            ),
            "仅顺序不同,stamp 相同,不该触发重开"
        );
        assert!(!search_source_globs_changed(&with(None), &with(None)));
    }

    /// 两个 adapter 的构造必须是同一个函数,不是两份「碰巧一致」的实现 ——
    /// 这里直接冒烟一下 weights_for_vault 本身是幂等、纯函数式的读取。
    #[test]
    fn weights_for_vault_is_deterministic_for_the_same_settings() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"searchWeights": {"source": 1.4}}"#);
        assert_eq!(weights_for_vault(d.path()), weights_for_vault(d.path()));
    }

    /// `weights_from`/`weights_for_vault` 的独立回落规则要覆盖 `SearchWeights`
    /// 直接构造(而不只是 JSON 字符串)的路径,免得只测到 serde 层。
    #[test]
    fn weights_from_a_directly_built_vault_settings_falls_back_per_component() {
        let vs = VaultSettings {
            search_weights: Some(SearchWeights { human: Some(-1.0), source: Some(1.7), ..Default::default() }),
            ..Default::default()
        };
        let w = weights_from(&vs);
        let default = Weights::default();
        assert_eq!(w.human, default.human, "负数非法,回落");
        assert_eq!(w.source, 1.7, "合法显式值生效");
        assert_eq!(w.derived, default.derived);
        assert_eq!(w.unlabeled, default.unlabeled);
    }

    /// 设置文件里的 attention 一路走到 Weights;0 必须活着到底。
    #[test]
    fn attention_weight_round_trips_from_settings() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"searchWeights": {"attention": 0}}"#);
        assert_eq!(weights_for_vault(d.path()).attention, 0.0, "用户关掉它就得关掉");

        write_settings(d.path(), r#"{"searchWeights": {"attention": 0.8}}"#);
        assert_eq!(weights_for_vault(d.path()).attention, 0.8);

        write_settings(d.path(), r#"{}"#);
        assert_eq!(
            weights_for_vault(d.path()).attention,
            searchidx::query::Weights::default().attention
        );
    }
}
