//! 两个只读工具。检索本身一行都不在这里 —— 全部委托 `cli::search::execute`,
//! 与 `notemd search --json` 是同一份数据的两个渲染。

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::cli::search::{execute, hit_to_json, SearchContext};
use crate::mcp::roots;
use crate::sotvault::vault_id;

/// Byte budget for the serialized `hits` array of a single `search` response.
/// A different axis from `limit` (which caps *hit count*; `0` there still
/// means "no cap", unchanged by this) — an agent cannot consume an unbounded
/// response any better than the old GUI-thread rebuild lock could produce one
/// quickly: `limit: 0` against a large vault used to build one enormous JSON
/// line, materialised while the index lock was held, only for the shell's 5s
/// timeout to fire mid-transfer and hand the agent nothing anyway. Stop
/// appending once the running total would cross this and say so via
/// `truncated: true` — visible to the agent, not a silent trim. 256 KiB
/// comfortably holds a few hundred ordinary hits.
pub(crate) const MAX_HITS_PAYLOAD_BYTES: usize = 256 * 1024;

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
    /// `(vault_id, mount_json, vault_id_error)`.
    ///
    /// `vault_id::ensure`'s only failure mode is being unable to write
    /// `.notemd/vault-id` (e.g. a read-only vault). This used to fall back to
    /// `""` via `unwrap_or_default()`, which then fed a bogus empty id into
    /// `roots::classify` — reporting `mismatched` for a reason that has
    /// nothing to do with any mount, and since a failing write regenerates
    /// nothing (there is no id to persist), two responses in one session
    /// could each independently fail and still both show `""`, which merely
    /// *looks* stable while silently violating "generated once, never
    /// changes" the moment the write starts succeeding mid-session and a
    /// real id appears (finding 9). Surface the failure explicitly instead:
    /// `mount` is forced to `unknown` with advice naming the real problem,
    /// and the raw error rides along for whoever is debugging it rather than
    /// being disguised as a mount mismatch.
    fn identity(&self) -> (String, Value, Option<String>) {
        match vault_id::ensure(&self.vault_root) {
            Ok(id) => {
                let (status, matched) = roots::classify(self.roots.as_deref(), &id);
                (id, roots::to_json(status, matched), None)
            }
            Err(e) => {
                let mount = json!({
                    "status": "unknown",
                    "matched_root": null,
                    "advice": "vault_id could not be determined (see vault_id_error) — \
                               mount status is unknown, not mismatched.",
                });
                (String::new(), mount, Some(e.to_string()))
            }
        }
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

    let (id, mount, vault_id_error) = env.identity();

    // Byte-budget truncation: an independent axis from `limit` above. Always
    // keeps at least one hit even if it alone exceeds the budget — telling
    // an agent "here's one, and it's huge" is more useful than telling it
    // "here's nothing".
    let mut hits: Vec<Value> = Vec::new();
    let mut hits_bytes = 0usize;
    let mut truncated = false;
    for h in &outcome.hits {
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
        let v_bytes = serde_json::to_vec(&v).map(|b| b.len()).unwrap_or(0);
        if !hits.is_empty() && hits_bytes + v_bytes > MAX_HITS_PAYLOAD_BYTES {
            truncated = true;
            break;
        }
        hits_bytes += v_bytes;
        hits.push(v);
    }

    // Single construction point for the envelope, shared with
    // `notemd search --json`'s `print_json` — see `envelope_json`'s doc
    // comment (finding 5). MCP-specific fields (`vault_id`/`mount`/
    // `truncated`/`vault_id_error`) are layered on top, not folded into the
    // shared function, since the CLI has no business knowing about any of
    // them.
    let envelope = crate::cli::search::envelope_json(&outcome.query, outcome.route, outcome.took_ms, hits);
    let mut obj = match envelope {
        Value::Object(m) => m,
        _ => unreachable!("envelope_json always returns a JSON object"),
    };
    obj.insert("vault_id".to_string(), json!(id));
    obj.insert("mount".to_string(), mount);
    obj.insert("truncated".to_string(), json!(truncated));
    obj.insert("vault_id_error".to_string(), json!(vault_id_error));
    Ok(Value::Object(obj))
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
    let (id, mount, vault_id_error) = env.identity();
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
        "vault_id_error": vault_id_error,
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
        // `env_with_fixture` seeds exactly one file (`notes/a.md`) —
        // `.is_some()` passes for any number including a silently-wrong one
        // (e.g. 0 from a build that never ran); assert the real count
        // (finding 11).
        assert_eq!(out["entry_count"].as_u64(), Some(1));
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

    /// finding 5: `search()`'s envelope (`query`/`route`/`took_ms`/`total`/
    /// `hits`) must come from the exact same construction the CLI's
    /// `print_json` uses (`cli::search::envelope_json`), not a hand-written
    /// second copy. Recomputes the envelope independently — straight off
    /// `execute()` and `envelope_json`, bypassing `search()` entirely — and
    /// asserts the two agree. If `search()` ever reverts to hand-writing
    /// these keys, a field added to one copy and not the other stops making
    /// this fail silently, which is the whole point.
    #[test]
    fn search_envelope_matches_the_shared_construction_cli_json_uses() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();

        let opts = crate::cli::search::scan_options_for(&env.vault_root);
        let guard = env.index.lock().unwrap();
        let ctx = SearchContext { root: &env.vault_root, index: guard.as_ref(), opts: &opts };
        let outcome = execute(&ctx, "zebraquux", 20);
        let hits: Vec<Value> = outcome.hits.iter().map(hit_to_json).collect();
        let expected = crate::cli::search::envelope_json(&outcome.query, outcome.route, outcome.took_ms, hits);

        assert_eq!(out["query"], expected["query"]);
        assert_eq!(out["route"], expected["route"]);
        assert_eq!(out["total"], expected["total"]);
        assert_eq!(out["hits"], expected["hits"], "hits must be byte-for-byte the same shape as the CLI's --json output");
    }

    /// finding 9: a `vault_id` write failure must be legible, not disguised
    /// as a mount mismatch. Blocks `.notemd/` from ever being created as a
    /// directory (a plain file sits at that path), forcing `vault_id::ensure`
    /// to fail on every call — the read-only-vault case the finding
    /// describes. `mount.status` must stay `unknown` (this has nothing to do
    /// with any mount) and the real error must ride along in
    /// `vault_id_error`, for both tools.
    #[test]
    fn vault_id_write_failure_is_legible_not_disguised_as_a_mount_mismatch() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".notemd"), b"not a directory").unwrap();
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

        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        assert_eq!(out["vault_id"], "", "no legitimate id exists to report — must not invent one");
        assert_eq!(out["mount"]["status"], "unknown", "a write failure is not a mount problem");
        assert!(out["vault_id_error"].as_str().is_some(), "the failure must be visible in the response: {out}");

        let info = vault_info(&env).unwrap();
        assert_eq!(info["vault_id"], "");
        assert_eq!(info["mount"]["status"], "unknown");
        assert!(info["vault_id_error"].as_str().is_some(), "{info}");
    }

    /// finding 4: `limit: 0` still means "no cap" on hit *count*, but the
    /// serialized payload is capped on a separate, byte-based axis — an
    /// agent cannot consume an unbounded response any better than the old
    /// held-lock construction could produce one before a client's timeout
    /// fired. 80 hits, each padded with a long context line so a handful of
    /// them alone cross `MAX_HITS_PAYLOAD_BYTES`, proves truncation actually
    /// engages (not just that the field exists) and that the response says
    /// so rather than trimming silently.
    #[test]
    fn search_caps_the_serialized_payload_and_reports_truncation() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        let padding = "x".repeat(2000);
        for i in 0..80 {
            std::fs::write(
                d.path().join(format!("notes/n{i}.md")),
                format!("# T{i}\n\n{padding}\n\nzebraquux 出现在第 {i} 篇\n\n{padding}\n"),
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

        // `limit: 0` (no cap on count) + generous context (each hit carries
        // the padded neighbour lines) so the byte budget, not the count cap,
        // is what stops this.
        let out = search(&env, &json!({ "query": "zebraquux", "limit": 0, "context": 4 })).unwrap();
        let hits = out["hits"].as_array().unwrap();
        assert!(hits.len() < 80, "the byte budget must have stopped this short of every match: got {}", hits.len());
        assert!(!hits.is_empty(), "must return at least what fits, not nothing");
        assert_eq!(out["truncated"], true, "truncation must be a visible field, not a silent trim");
        assert_eq!(out["total"], json!(hits.len()), "total must agree with what was actually returned");

        let serialized_hits_bytes = serde_json::to_vec(&out["hits"]).unwrap().len();
        assert!(
            serialized_hits_bytes <= MAX_HITS_PAYLOAD_BYTES + 64 * 1024,
            "serialized hits ({serialized_hits_bytes} bytes) must stay within a small margin of the budget \
             ({MAX_HITS_PAYLOAD_BYTES} bytes) — the one-extra-hit-over-budget allowance, not runaway growth"
        );
    }

    /// The other side of the same axis: a query with few enough/small enough
    /// hits to stay under budget must NOT report truncation — the byte cap
    /// must not fire on ordinary-sized responses.
    #[test]
    fn small_search_response_is_not_marked_truncated() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        assert_eq!(out["truncated"], false);
    }
}
