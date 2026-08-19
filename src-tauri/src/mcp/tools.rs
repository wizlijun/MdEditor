//! 两个只读工具。检索本身一行都不在这里 —— 全部委托 `cli::search::execute`,
//! 与 `notemd search --json` 是同一份数据的两个渲染。

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::cli::search::{execute, hit_to_json, SearchContext};
use crate::mcp::roots;
use crate::sotvault::vault_id;

pub struct ToolEnv {
    pub vault_root: PathBuf,
    pub index: crate::search::IndexHandle,
    /// client 声明的 roots;`None` = 未声明能力。
    pub roots: Option<Vec<String>>,
    /// `open_vault` 后台线程当前所在阶段。`None` 表示宿主没有提供 ——
    /// 这本身要作为一种独立状态上报,不能悄悄并入某个失败态(见
    /// `describe_open_phase`)。一个后续任务会在 GUI 里用真实的
    /// `search::OpenState::get()` 填这个字段;这里不负责接线。
    pub open_phase: Option<crate::search::OpenPhase>,
}

impl ToolEnv {
    fn identity(&self) -> (String, Value) {
        let id = vault_id::ensure(&self.vault_root).unwrap_or_default();
        let (status, matched) = roots::classify(self.roots.as_deref(), &id);
        (id, roots::to_json(status, matched))
    }
}

pub fn search(env: &ToolEnv, args: &Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "search 需要 query 参数".to_string())?;
    if query.trim().is_empty() {
        return Err("query 不能为空".to_string());
    }
    // `0` 是「不设上限」的哨兵,与 CLI 的 `--limit 0` / `--all` 同义。
    let limit = match args.get("limit").and_then(|v| v.as_u64()) {
        Some(0) => searchidx::NO_LIMIT,
        Some(n) => n as usize,
        None => 20,
    };
    let context = args.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    let opts = crate::cli::search::scan_options_for(&env.vault_root);
    let guard = env.index.lock().map_err(|_| "索引锁中毒".to_string())?;
    let ctx = SearchContext { root: &env.vault_root, index: guard.as_ref(), opts: &opts };
    let outcome = execute(&ctx, query, limit);
    drop(guard);

    let (id, mount) = env.identity();
    let hits: Vec<Value> = outcome
        .hits
        .iter()
        .map(|h| {
            let mut v = hit_to_json(h);
            if context > 0 {
                if let Some(lines) =
                    crate::cli::search::context_lines_public(&env.vault_root, h, context)
                {
                    v["context"] = json!(lines
                        .iter()
                        .map(|(n, t)| json!({ "line": n, "text": t }))
                        .collect::<Vec<_>>());
                }
            }
            v
        })
        .collect();

    Ok(json!({
        "vault_id": id,
        "mount": mount,
        "query": outcome.query,
        "route": outcome.route.as_str(),
        "took_ms": outcome.took_ms,
        "total": hits.len(),
        "hits": hits,
    }))
}

/// Renders [`crate::search::OpenPhase`] (plus the "host didn't supply one"
/// case, which is distinct from all four of its variants) as a short
/// lowercase string an LLM reads at a glance, alongside the backend's own
/// error text when there is one. Never invents a state the enum doesn't have
/// — see the review finding this responds to.
fn describe_open_phase(phase: Option<&crate::search::OpenPhase>) -> (&'static str, Option<String>) {
    use crate::search::OpenPhase;
    match phase {
        None => ("not_supplied", None),
        Some(OpenPhase::Idle) => ("idle", None),
        Some(OpenPhase::Opening) => ("opening", None),
        Some(OpenPhase::Ready) => ("ready", None),
        Some(OpenPhase::Failed(msg)) => ("failed", Some(msg.clone())),
    }
}

pub fn vault_info(env: &ToolEnv) -> Result<Value, String> {
    let (id, mount) = env.identity();
    let guard = env.index.lock().map_err(|_| "索引锁中毒".to_string())?;
    // `entry_count`/`indexed_at` still come straight off the index itself
    // (independent of `open_phase`, which is a different axis — "is the
    // background open thread done" vs. "what did the last completed build
    // find"). `stats().built_at` is already an `Option<String>`; wrapping it
    // in another `Some(..)` here used to produce `Option<Option<String>>`,
    // which serializes fine but is a trap for the next reader — read it
    // straight through instead.
    let (entry_count, indexed_at) = match guard.as_ref().and_then(|i| i.stats().ok()) {
        Some(s) => (s.files, s.built_at),
        None => (0, None),
    };
    // Match `search()`'s discipline: drop the lock before building the
    // response, even though nothing slow happens under it here.
    drop(guard);
    let (index_state, index_error) = describe_open_phase(env.open_phase.as_ref());
    Ok(json!({
        "vault_id": id,
        // 本机视角绝对路径,**仅供人核对**;agent 不得用于路径拼接。
        "vault_root": env.vault_root.to_string_lossy(),
        "entry_count": entry_count,
        "indexed_at": indexed_at,
        // 把「无 vault / 后台正在开 / 已就绪 / 上次打开失败 / 宿主未提供」
        // 五种状态区分开来 —— 否则前四种在 entry_count:0 上全部塌缩成
        // 同一个不可区分的答案(review finding 1)。
        "index_state": index_state,
        "index_error": index_error,
        "notemd_version": env!("CARGO_PKG_VERSION"),
        "mount": mount,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env_with_fixture() -> (tempfile::TempDir, ToolEnv) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        std::fs::write(d.path().join("notes/a.md"), "# T\n\nzebraquux 在这里\n").unwrap();
        let opts = crate::cli::search::scan_options_for(d.path());
        let mut idx = searchidx::SearchIndex::open(d.path(), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let env = ToolEnv {
            vault_root: d.path().to_path_buf(),
            index: std::sync::Arc::new(std::sync::Mutex::new(Some(idx))),
            roots: None,
            open_phase: None,
        };
        (d, env)
    }

    /// 25 个匹配文件——比默认上限 20 多,好让「默认给 20」与「limit:0 给
    /// 全部」这两条断言都不可能空洞地通过(review finding 2)。
    fn env_with_many_matches(n: usize) -> (tempfile::TempDir, ToolEnv) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        for i in 0..n {
            std::fs::write(
                d.path().join(format!("notes/n{i}.md")),
                format!("# T{i}\n\nzebraquux 出现在第 {i} 篇\n"),
            )
            .unwrap();
        }
        let opts = crate::cli::search::scan_options_for(d.path());
        let mut idx = searchidx::SearchIndex::open(d.path(), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let env = ToolEnv {
            vault_root: d.path().to_path_buf(),
            index: std::sync::Arc::new(std::sync::Mutex::new(Some(idx))),
            roots: None,
            open_phase: None,
        };
        (d, env)
    }

    #[test]
    fn search_returns_relative_paths_only() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        let hits = out["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        for h in hits {
            let p = h["path"].as_str().unwrap();
            assert!(!p.starts_with('/'), "绝不返回绝对路径: {p}");
            assert!(!p.contains("/Users/"), "绝不泄漏本机路径: {p}");
        }
    }

    #[test]
    fn every_search_response_carries_identity() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        assert_eq!(out["vault_id"].as_str().unwrap().len(), 36);
        assert_eq!(out["mount"]["status"], "unknown");
    }

    #[test]
    fn vault_info_reports_identity_and_root() {
        let (d, env) = env_with_fixture();
        let out = vault_info(&env).unwrap();
        assert_eq!(out["vault_id"].as_str().unwrap().len(), 36);
        assert_eq!(out["vault_root"], d.path().to_string_lossy().to_string());
        assert!(out["entry_count"].as_u64().is_some());
    }

    #[test]
    fn missing_query_is_an_error() {
        let (_d, env) = env_with_fixture();
        assert!(search(&env, &json!({})).is_err());
    }

    #[test]
    fn limit_zero_means_no_cap() {
        // 25 篇命中,超过默认上限 20:`.as_array().is_some()` 对 `[]` 一样
        // 为真,不能证明「没设上限」;必须比出确切数量,一条回归把
        // `limit:0` 映射成字面 0 行才会被这条测试抓住。
        let (_d, env) = env_with_many_matches(25);

        let default_out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        assert_eq!(
            default_out["hits"].as_array().unwrap().len(),
            20,
            "缺省 limit 必须是 20"
        );

        let unlimited_out = search(&env, &json!({ "query": "zebraquux", "limit": 0 })).unwrap();
        assert!(
            unlimited_out["hits"].as_array().unwrap().len() > 20,
            "limit:0 必须突破默认上限,拿到全部 25 条命中"
        );
    }

    #[test]
    fn vault_info_reports_index_state_and_distinguishes_not_supplied() {
        let (_d, mut env) = env_with_fixture();

        // 宿主没给 open_phase:必须是独立的 "not_supplied",不能悄悄冒充
        // 某个失败态或直接不出现。
        let out = vault_info(&env).unwrap();
        assert_eq!(out["index_state"], "not_supplied");
        assert!(out["index_error"].is_null());

        // 给一个真实阶段,必须序列化成不同的值,且 indexed_at 不再是
        // 嵌套 Option。
        env.open_phase = Some(crate::search::OpenPhase::Ready);
        let out = vault_info(&env).unwrap();
        assert_eq!(out["index_state"], "ready");
        assert_ne!(out["index_state"], "not_supplied");

        env.open_phase = Some(crate::search::OpenPhase::Failed("boom".to_string()));
        let out = vault_info(&env).unwrap();
        assert_eq!(out["index_state"], "failed");
        assert_eq!(out["index_error"], "boom");
    }
}
