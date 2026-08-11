//! The design spec's §7 acceptance table, as tests. These are the definition of
//! the feature: everything else is implementation detail.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use searchidx::{ScanOptions, SearchIndex};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn open_temp(vault: &Path) -> (tempfile::TempDir, SearchIndex) {
    let d = tempfile::tempdir().unwrap();
    let idx = SearchIndex::open_at(vault, &d.path().join("index.db"), "sync").unwrap();
    (d, idx)
}

/// spec §7:已知事实回归集(见 tests/fixtures/retrievability.json),CI 常跑。
/// 分词盲区与排序回归的守门员 —— 见该文件与 task-11 报告里对「100 条」目标的
/// 诚实核算(语料是刻意的小型合成 fixture,不是 spec 锚定的 8,826 文件真实
/// vault,硬凑到 100 条会用近重复条目稀释这份清单的可信度)。
///
/// **两种断言,一份 fixture。** 每条用例都断言召回(`expect_path` 必须出现在
/// 前 20 条里);带 `outranks_path` 的用例**额外**断言顺序:`expect_path` 必须
/// 排在 `outranks_path` 前面。
///
/// 为什么要有第二种:origin 分级(spec `2026-08-11-md-origin-tiering-design.md`
/// §4)是**纯重排序** —— 它一条也不会让文档从结果里消失,只改前后。原本这里
/// 只有 `hits.iter().any(|h| h.path == want)`,对重排序**结构性地看不见**:
/// 三档乘数全部改成 ×1.0,这 50 条依旧全绿。所以顺序类主张(「你写的排在 AI
/// 摘要前」)在这个 fixture 格式里根本写不出来,只能靠人去 diff 有序命中表。
///
/// 选择的形式是**扩展既有 fixture**,而不是新开一个平行的顺序测试文件:
/// 顺序主张和召回主张说的是同一批查询在同一个语料上的行为,拆成两份会各自
/// 长出各自的语料假设然后漂移(前置项目已经吃过一次「两个乘数一起推同一
/// 方向」的亏,根因就是断言散落在不同地方)。新键全部可选,原有 50 条一个
/// 字都不用改 —— 没有 `outranks_path` 的用例行为与之前完全一致。
///
/// `expect_text` / `outranks_text` 是**可选的块级选择器**:同一个文件里可能有
/// 多个块命中同一个词(`.note.md` 的人工节点 vs agent 写的答复节点),只按
/// path 选会选到 hits 里靠前的那个,断言就变成同一条命中和自己比。给出
/// 子串时取第一条 path 与 text 都匹配的命中。
///
/// 顺序断言要求**比较对象本身也被召回**。「B 根本没出现所以 A 赢了」不是
/// 这条断言想固化的事实 —— 那是一次召回变化,必须红,不能被当成顺序正确
/// 而静默放过。
#[test]
fn retrievability_regression_set_is_fully_recalled_and_correctly_ordered() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();

    let cases: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/retrievability.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!cases.is_empty(), "the regression set must not be empty");

    let mut failures = Vec::new();
    for case in &cases {
        let q = case["query"].as_str().unwrap();
        let want = case["expect_path"].as_str().unwrap();
        let want_text = case["expect_text"].as_str();
        let (hits, route) = idx.search(q, 20).unwrap();
        let seen = |hits: &[searchidx::Hit]| {
            hits.iter().take(3).map(|h| h.path.clone()).collect::<Vec<_>>().join(", ")
        };
        let Some(want_at) = position_of(&hits, want, want_text) else {
            failures.push(format!(
                "  {q:?} → expected {want}{}, got [{}] (route {})",
                want_text.map(|t| format!(" containing {t:?}")).unwrap_or_default(),
                seen(&hits),
                route.as_str()
            ));
            continue;
        };
        let Some(below) = case["outranks_path"].as_str() else { continue };
        let below_text = case["outranks_text"].as_str();
        let Some(below_at) = position_of(&hits, below, below_text) else {
            failures.push(format!(
                "  {q:?} → order case, but its comparison target {below} was not recalled at all; \
                 an absent target cannot prove {want} outranks it. got [{}]",
                seen(&hits)
            ));
            continue;
        };
        if want_at >= below_at {
            failures.push(format!(
                "  {q:?} → {want} (#{want_at}) must outrank {below} (#{below_at}); ranked order: [{}]",
                hits.iter()
                    .map(|h| format!("{}:{} {:.6}", h.path, h.line, h.score))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    assert!(failures.is_empty(), "retrievability regressions:\n{}", failures.join("\n"));
}

/// Rank of the first hit on `path` (optionally the first one whose text also
/// contains `text` — see the fixture-format note on the test above).
fn position_of(hits: &[searchidx::Hit], path: &str, text: Option<&str>) -> Option<usize> {
    hits.iter().position(|h| h.path == path && text.map_or(true, |t| h.text.contains(t)))
}

/// spec §7:删库重建逐字节一致(同一 tokenizer_id 下)。索引=纯函数的验收形式.
///
/// `SearchIndex` deliberately does not expose its `rusqlite::Connection` (no
/// real caller — Tauri command, CLI, watcher — ever needs raw SQL access), so
/// rather than widen the facade's public surface just for this one test, both
/// indexes are built directly through `store`/`scan` (the same layer
/// `SearchIndex::rebuild` itself calls) and compared by reading back the
/// actual stored rows — path, line range, level, breadcrumb, text, and the
/// provenance flags — in a deterministic order. That is a stronger property
/// than the brief's original four-query `dump()`: two builds could agree on
/// every probe query while still disagreeing on rows no probe happened to
/// touch (a redundant rollup, a duplicate row, a byte of drift in a
/// breadcrumb) — this reads everything, not a sample.
#[test]
fn rebuilding_from_scratch_is_deterministic() {
    fn dump(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT f.path, b.line_start, b.line_end, b.level, b.breadcrumb, b.text, \
                        b.is_annotation, b.agent_by
                 FROM blocks b JOIN files f ON f.id = b.file_id
                 ORDER BY f.path, b.line_start, b.level, b.text",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok(format!(
                "{}|{}|{}|{}|{}|{}|{}|{}",
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            ))
        })
        .unwrap()
        .map(|x| x.unwrap())
        .collect()
    }

    let d1 = tempfile::tempdir().unwrap();
    let mut c1 = searchidx::store::open(&d1.path().join("index.db"), "v", "sync").unwrap();
    searchidx::scan::build_full(&mut c1, &corpus(), &ScanOptions::default(), None).unwrap();

    let d2 = tempfile::tempdir().unwrap();
    let mut c2 = searchidx::store::open(&d2.path().join("index.db"), "v", "sync").unwrap();
    searchidx::scan::build_full(&mut c2, &corpus(), &ScanOptions::default(), None).unwrap();

    let dump1 = dump(&c1);
    let dump2 = dump(&c2);
    assert!(!dump1.is_empty(), "the corpus must actually produce rows");
    assert_eq!(dump1, dump2);
}

/// spec §7:GUI+CLI 并发写必须自然收敛,不靠锁协商。两个连接交错写同一批文件,
/// 结果必须与单进程重建一致 —— 这条测试是「免 IPC、免锁」设计主张背后唯一的
/// 挡箭牌:`store::replace_file`/`remove_file` 必须先删后插(delete-then-insert),
/// 而不是追加或遗留孤儿行。手动改坏过 `remove_file`(注释掉它对 `blocks`/
/// `blocks_fts` 的两条 DELETE,只留 `links`/`files`)重跑过这条测试:三次交错
/// 写入(`a`/`b`/`a`)在第二次 `index_one` 时就会因残留的旧 `blocks` 行触发
/// `FOREIGN KEY constraint failed` 而 panic——比"计数不相等"更早、更硬地红,
/// 说明这条测试对"先删后插"退化不仅敏感,而且不会被悄悄放过。详见 task-11
/// 报告的「mutation-checked」小节。
#[test]
fn two_writers_converge_without_coordination() {
    let vault = corpus();
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("index.db");

    let mut a = SearchIndex::open_at(&vault, &db, "sync").unwrap();
    a.rebuild(&ScanOptions::default()).unwrap();

    let mut files: Vec<String> = Vec::new();
    collect_markdown_files(&vault, &vault, &mut files);
    assert!(!files.is_empty(), "the corpus must contain markdown files to interleave writes over");

    let mut b = SearchIndex::open_at(&vault, &db, "sync").unwrap();
    for rel in &files {
        a.index_one(rel, &ScanOptions::default()).unwrap();
        b.index_one(rel, &ScanOptions::default()).unwrap();
        a.index_one(rel, &ScanOptions::default()).unwrap();
    }
    let s = a.stats().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut fresh = SearchIndex::open_at(&vault, &d2.path().join("index.db"), "sync").unwrap();
    fresh.rebuild(&ScanOptions::default()).unwrap();
    let fresh_stats = fresh.stats().unwrap();
    assert_eq!(s.files, fresh_stats.files);
    assert_eq!(s.blocks, fresh_stats.blocks, "interleaved writes must not duplicate rows");
}

/// Recurse the corpus directory for `.md`/`.note.md` files, vault-relative
/// with `/` separators — `scan::walk` is private, and `read_dir` alone (the
/// brief's original approach) misses the corpus's nested directories
/// (`concepts/`, `people/`, `docs/`, ...), which would silently interleave
/// writes over only the handful of files sitting at the corpus root instead
/// of the whole fixture set.
fn collect_markdown_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(root, &path, out);
        } else if path.extension().is_some_and(|x| x == "md") {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push(rel);
        }
    }
}

/// spec §7:无变更 sweep < 300ms。CLI 默认路径的可用性下限.
///
/// The corpus (a few dozen tiny files) is nowhere near the spec's 10k/150MB
/// anchor, so the real regression this guards against is qualitative, not a
/// timing curve: a sweep that stops trusting the stat fast-path and
/// re-reads/re-hashes (or worse, re-parses) every file on every call. The
/// primary assertion is that property (`files_indexed == 0`); the wall-clock
/// bound is a generous backstop against that specific catastrophic
/// regression, not a tight performance budget this tiny fixture could ever
/// meaningfully measure.
#[test]
fn an_unchanged_sweep_is_fast() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let t = Instant::now();
    let s = idx.sweep(&ScanOptions::default(), None).unwrap();
    assert_eq!(s.files_indexed, 0, "an unchanged vault must be a sweep no-op");
    assert!(t.elapsed() < Duration::from_millis(300), "sweep took {:?}", t.elapsed());
}

/// spec §7:查询 p50 < 10ms(索引热)。Median over 20 warm runs so one slow
/// tick (a GC pause, a scheduler hiccup) can't flip the assertion — the
/// property under test is "queries do not regress into a full scan", and a
/// catastrophic instance of that (e.g. `fts_search` losing its index and
/// degrading to a table scan) would blow well past this budget even on a
/// corpus this small, while ordinary noise on one iteration will not.
///
/// Review round 1 finding: the timing bound alone is a bare wall-clock gate
/// with no property assertion beside it — on a 32-file corpus, even a full
/// unindexed `LIKE` scan over `blocks.text` (the exact catastrophic
/// regression this test exists to catch — `fts_search` silently losing its
/// index and every query degrading to `like_search`) would very plausibly
/// still finish under 10ms, so the timing bound alone would not reliably
/// catch it. Asserting `route.as_str() == "t1-fts"` pins the MECHANISM
/// (queries actually take the indexed path) independent of how fast that
/// mechanism happens to run on this corpus; the timing assertion stays as a
/// secondary backstop against slowness within that mechanism.
///
/// Review round 2 finding: the round-1 fix above still could not fail for
/// the right reason. `search()`'s final fallthrough is `Ok((Vec::new(),
/// Route::Fts))` — reached whenever `fts_search` returns empty AND
/// `needs_scan_fallback` is false. `"note"` is 4-char ASCII with no Han, so
/// `needs_scan_fallback("note")` is false; a `fts_search` that was forced to
/// unconditionally return `Ok(Vec::new())` (simulating "the FTS index is
/// gone") still satisfies `route.as_str() == "t1-fts"` — the route string
/// alone does not distinguish "FTS answered" from "FTS returned nothing and
/// nothing caught it." Two additions close that: (1) `!hits.is_empty()` on
/// the warm query itself, which directly catches the empty-fallthrough case;
/// (2) a second probe (`铁`, a single Han char that is a genuine dictionary
/// blind spot — see `people/2026-07-01-oov-name.md` and the `慕`/`李慕白`
/// pair in `query.rs`'s own tests) that MUST route to `t1-scan` with real
/// hits, proving the route label is load-bearing in both directions rather
/// than a static string a broken index could still emit.
#[test]
fn warm_queries_are_fast() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();

    let (hits, warm_route) = idx.search("search", 20).unwrap();
    assert_eq!(
        warm_route.as_str(),
        "t1-fts",
        "a warm query must resolve via the FTS index, not degrade to the bounded scan fallback"
    );
    assert!(
        !hits.is_empty(),
        "a warm 'search' query must return real hits — an index whose FTS path silently returns \
         nothing would still report route t1-fts via search()'s empty fallthrough, so the route \
         label alone does not catch that regression"
    );

    // Pin the fallback direction too: a query that genuinely misses the FTS
    // index (a dictionary blind spot) must route to t1-scan and still find
    // real hits — proving t1-fts vs t1-scan is a meaningful distinction, not
    // a label a broken index could produce either way.
    let (scan_hits, scan_route) = idx.search("铁", 20).unwrap();
    assert_eq!(
        scan_route.as_str(),
        "t1-scan",
        "a genuine dictionary blind spot must route to the bounded scan fallback"
    );
    assert!(!scan_hits.is_empty(), "the scan fallback must actually find the out-of-vocabulary hit");

    let mut times: Vec<u128> = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let _ = idx.search("search", 20).unwrap();
        times.push(t.elapsed().as_micros());
    }
    times.sort_unstable();
    assert!(times[times.len() / 2] < 10_000, "p50 {}µs", times[times.len() / 2]);
}

/// spec §7 + §4:批注(`type:: annotation`)必须排在同一文件里的普通内容之前.
///
/// Review round 1 finding: the original version of this test used one
/// fixture where `type:: annotation` sat on one sibling AND `by:: claude/1`
/// sat on the other, so the annotation boost (×1.2) and the agent-authored
/// penalty (×0.85) both pushed the SAME direction at once — either multiplier
/// alone was sufficient to produce the expected order, so a silent regression
/// of `is_annotation` to a no-op (while the `agent_by` penalty kept working)
/// would have stayed green. That is exactly the failure mode `query.rs`'s own
/// `annotations_outrank_agent_authored_blocks` doc comment already warns
/// about for a stable-sort tie-break — the same shape of gap, one level up.
///
/// This version isolates `is_annotation` alone:
/// `concepts/2026-07-20-isolate-annotation.note.md` has two siblings that are
/// otherwise byte-for-byte parallel (`annoisotoken alpha ...` /
/// `annoisotoken bravo ...`, same word count, neither `by::`-tagged), so the
/// two blocks' raw bm25 is an exact tie — verified empirically: printing
/// `Hit::score` with the `is_annotation` multiplier temporarily no-op'd in
/// `score_of` showed the two scores identical to 9 decimal places. The
/// annotation is deliberately placed on the SECOND bullet (`bravo`), not the
/// first: `blocks_fts`'s own tie-break (observed, not assumed — see the
/// mutation-check note below) favors the first-inserted row, so if the
/// annotation sat on `alpha` this test could pass on tie-break luck alone,
/// the exact gap being closed here. Putting it on the tie-break-*disfavored*
/// side means the ×1.2 boost is the only thing that can produce the correct
/// order.
#[test]
fn annotation_boost_outranks_plain_content_end_to_end() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let (hits, _) = idx.search("annoisotoken", 20).unwrap();
    let line_hits: Vec<_> = hits.iter().filter(|h| h.level == "line").collect();
    let annotated = line_hits.iter().position(|h| h.text.contains("bravo")).expect("annotation hit missing");
    let plain = line_hits.iter().position(|h| h.text.contains("alpha")).expect("plain hit missing");
    assert!(
        annotated < plain,
        "type:: annotation must outrank otherwise-identical plain content: {line_hits:?}"
    );
}

/// spec §7 + §4:AI 撰写(`by:: <非 human:>`)内容必须排在同一文件里的普通内容
/// 之后。Isolates `agent_by` alone, the sibling to the test above — see its
/// doc comment for why isolation (not the original combined fixture) matters.
///
/// `concepts/2026-07-21-isolate-agentby.note.md`'s two blocks are otherwise
/// parallel and tie exactly on raw bm25 (same verification method as above).
/// `by:: claude/1` is deliberately on the FIRST bullet (`alpha`) — the
/// tie-break-favored side (observed, see the mutation-check note below) — so
/// only the ×0.85 penalty can make the second bullet (`bravo`, plain) win.
#[test]
fn agent_authored_content_is_penalized_end_to_end() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let (hits, _) = idx.search("agentbytoken", 20).unwrap();
    let line_hits: Vec<_> = hits.iter().filter(|h| h.level == "line").collect();
    let plain = line_hits.iter().position(|h| h.text.contains("bravo")).expect("plain hit missing");
    let agent = line_hits.iter().position(|h| h.text.contains("alpha")).expect("agent-authored hit missing");
    assert!(
        plain < agent,
        "agent-authored (by:: claude/1) content must rank below otherwise-identical plain content: {line_hits:?}"
    );
}

/// spec §7 + §4:`human_verified` 内容必须排在同一查询下未核实内容之前 ——
/// 端到端版本,证明 frontmatter 的 `verified: by: human:...` 一路传导到
/// `files.human_verified` 再到排序,而不只是 `score_of` 的加成系数本身对.
///
/// Review round 1 finding: the original fixture pair
/// (`docs/2026-03-01-verified-fact.md` / `docs/2026-03-02-unverified-fact.md`,
/// kept in the corpus unchanged for `retrievability.json`) differ in length
/// by about 40% — `bm25()`'s own length normalization plausibly favored the
/// shorter file regardless of the boost, an uncontrolled confound. This
/// version uses a purpose-built pair
/// (`docs/2026-07-22-verified-marker-a.md` / `-b.md`) with identical word
/// counts (`humanverifiedtoken review alpha/bravo steady content marker
/// today`), verified empirically to tie exactly on raw bm25 with the boost
/// no-op'd. `verified:` sits on `-b.md`, not `-a.md`: `-a.md` sorts first
/// alphabetically (`walk()` processes candidates in sorted path order) and
/// is the tie-break-favored side (observed, see the mutation-check note
/// below), so only the ×1.1 boost can make `-b.md` win.
///
/// A pure `score_of`-level pin (`score_of_boosts_human_verified_content` in
/// `query.rs`, added in this same review round) is the primary, fixture- and
/// bm25-independent guard for the ×1.1 multiplier; this test additionally
/// proves the boost survives the full pipeline (frontmatter parsing →
/// `files.human_verified` → ranking), the way the annotation/agent_by pair
/// above does for their multipliers.
///
/// **Origin tiering broke that isolation, and the `type: Note` line now in
/// both fixtures is what restores it** (task 5). Until then `-a.md` had no
/// frontmatter at all, so once `origin` started multiplying the score it
/// classified `Source` (rule 6) at ×0.9 while `-b.md`'s `verified:` block
/// classified `Human` (rule 3) at ×1.25 — a 1.39× gap pushing the same
/// direction as the ×1.1 under test. Measured, not assumed: with `r *= 1.1`
/// forced to `r *= 1.0`, this test still passed. Worse, the tier gap did not
/// even depend on `verified:` parsing surviving — with `verified:` unreadable
/// `-b.md` would fall to rule 7's `Derived` ×1.0 and still beat `-a.md`'s
/// ×0.9, so the pipeline claim in the paragraph above had quietly become
/// unfalsifiable. Giving BOTH files `type: Note` (rule 4 → `Human`) ties the
/// origin multiplier at ×1.25 on both sides, leaving `human_verified` the
/// only difference again; re-running the same mutation now fails this test,
/// as it must. Anything that changes either file's frontmatter must re-check
/// that the two still land in the SAME origin tier — see spec
/// `2026-08-11-md-origin-tiering-design.md` §8's "逐档隔离" requirement.
#[test]
fn human_verified_content_outranks_unverified_content_for_the_same_query() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let (hits, _) = idx.search("humanverifiedtoken", 20).unwrap();
    let verified =
        hits.iter().position(|h| h.path == "docs/2026-07-22-verified-marker-b.md").expect("verified doc missing");
    let unverified =
        hits.iter().position(|h| h.path == "docs/2026-07-22-verified-marker-a.md").expect("unverified doc missing");
    assert!(
        verified < unverified,
        "human_verified content must outrank otherwise-identical unverified content: {hits:?}"
    );
}

/// spec §7:保存 → 可检索 < 500ms。这里测的是"重索引一个文件"本身的成本,
/// 300ms 去抖之外还剩多少预算。
#[test]
fn reindexing_one_file_is_well_under_the_freshness_budget() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("a.md"), "before\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("index.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    std::fs::write(v.path().join("a.md"), "after brownfox\n").unwrap();
    let t = Instant::now();
    idx.index_one("a.md", &ScanOptions::default()).unwrap();
    let took = t.elapsed();
    assert!(!idx.search("brownfox", 5).unwrap().0.is_empty());
    assert!(took < Duration::from_millis(200), "single-file reindex took {took:?}");
}
