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

pub fn vault_info(env: &ToolEnv) -> Result<Value, String> {
    let (id, mount) = env.identity();
    let guard = env.index.lock().map_err(|_| "索引锁中毒".to_string())?;
    let (entry_count, indexed_at) = match guard.as_ref().and_then(|i| i.stats().ok()) {
        Some(s) => (s.files, Some(s.built_at)),
        None => (0, None),
    };
    Ok(json!({
        "vault_id": id,
        // 本机视角绝对路径,**仅供人核对**;agent 不得用于路径拼接。
        "vault_root": env.vault_root.to_string_lossy(),
        "entry_count": entry_count,
        "indexed_at": indexed_at,
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
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux", "limit": 0 })).unwrap();
        assert!(out["hits"].as_array().is_some());
    }
}
