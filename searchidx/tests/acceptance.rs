//! The design spec's §7 acceptance table, as tests. These are the definition of
//! the feature: everything else is implementation detail.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use searchidx::query::{Conventions, Hit, Weights};
use searchidx::{Limits, Query, ScanOptions, SearchIndex};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn open_temp(vault: &Path) -> (tempfile::TempDir, SearchIndex) {
    let d = tempfile::tempdir().unwrap();
    let idx = SearchIndex::open_at(vault, &d.path().join("index.db"), "sync").unwrap();
    (d, idx)
}

/// Open + rebuild an index over the corpus with `patterns` as the vault's
/// configured source globs. The stamp passed to `open_at` is the *same*
/// `SourceGlobs`'s own stamp, not an independently spelled string — see
/// `SearchIndex::open`'s doc comment for why those two must never be derived
/// separately.
fn open_temp_with_globs(patterns: &[String]) -> (tempfile::TempDir, SearchIndex) {
    let globs = searchidx::globs::parse(patterns);
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(&corpus(), &d.path().join("index.db"), &globs.stamp()).unwrap();
    idx.rebuild(&ScanOptions { source_globs: globs, ..Default::default() }).unwrap();
    (d, idx)
}

#[test]
fn typed_query_entry_matches_raw_ranking_and_preserves_multiword_filters() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("plain.md"), "target\n").unwrap();
    std::fs::write(
        v.path().join("summary.md"),
        "---\ntype: Book Summary\n---\ntarget\n",
    )
    .unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("index.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();
    let limits = Limits::full();
    let weights = Weights::default();
    let conventions = Conventions::default();

    let raw = idx
        .search_ranked("target", 20, &limits, &weights, &conventions)
        .unwrap();
    let parsed = searchidx::query::parse("target");
    let typed = idx
        .search_query_ranked(&parsed, 20, &limits, &weights, &conventions)
        .unwrap();
    assert_eq!(typed.route, raw.route);
    assert_eq!(typed.truncated, raw.truncated);
    assert_eq!(typed.deep_available, raw.deep_available);
    assert_eq!(
        typed
            .hits
            .iter()
            .map(|hit| (&hit.path, hit.line, hit.line_end, hit.score.to_bits()))
            .collect::<Vec<_>>(),
        raw.hits
            .iter()
            .map(|hit| (&hit.path, hit.line, hit.line_end, hit.score.to_bits()))
            .collect::<Vec<_>>()
    );

    let filtered = idx
        .search_query_ranked(
            &Query {
                terms: vec!["target".into()],
                types: vec!["Book Summary".into()],
                raw: "structured multi-word type".into(),
                ..Default::default()
            },
            20,
            &limits,
            &weights,
            &conventions,
        )
        .unwrap();
    assert!(!filtered.hits.is_empty());
    assert!(filtered.hits.iter().all(|hit| hit.path == "summary.md"));
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
///
/// **第三种断言(task 6),`expect_none`。** `origin:` 过滤器对非法值的约定是
/// 失败关闭(匹配零行,不是静默丢过滤器退回全部结果)—— 这条主张只能写成
/// 「零命中」,`expect_path` 的存在性检查表达不了。带 `expect_none: true` 的
/// 用例跳过 `expect_path`,改断言 `hits.is_empty()`。
///
/// **第四种断言(task 6 review round 1),`expect_not_path`。** 过滤器用例
/// (`origin:human` 之类)必须证明**排除**,而 `expect_path` 只证明召回 ——
/// 一个被整块禁用的过滤器,查询照样召回同一份文件(语料本来就没加过滤时也命中
/// 它),`expect_path` 单独出现时对此是假阳性。带 `expect_not_path` 的用例
/// 额外断言该 path 不在返回的命中里。
///
/// **第五种断言(task C-T12),`expect_line`。** `.srt`/`.vtt` 的引用契约是
/// 「时间码被剔出索引文本,但行号仍指向原文件的真实文本行」—— 一个把行号算到
/// 序号行或时间码行上的分块器,召回断言完全看不出来(文本照样命中),点开却会
/// 落在一行数字或一串时间码上。带 `expect_line` 的用例额外断言命中的
/// `Hit::line` 精确等于给定值。
///
/// **第六个可选键(task C-T12),`source_globs`。** 转写稿只有被用户的模式指定
/// 时才进索引(`scan::is_indexable`),「原始资料」与「未标注」的分野也由模式
/// 决定(`origin::derive` 规则 5′)—— 这两件事在 `ScanOptions::default()`
/// (空模式集,按 `SourceGlobs` 自己的契约匹配零个文件)下**根本无法表达**。
/// 带 `source_globs` 的用例跑在一个用该模式单独重建过的索引上;省略该键的用例
/// (既有 62 条,一字未改)仍跑在默认的空模式索引上。按模式集分组、每组只建一
/// 次索引,不是每条用例建一次。
///
/// **排序权重固定为 `Weights::default()`,而且显式传。** 这份回归集是**档位与
/// 过滤器类**排序主张的唯一裁判;它若跟着用户配置
/// (`search::options::weights_for_vault`)走就等于没有裁判 —— 一个把四档权重全
/// 填成 ×1.0 的 vault 会让每一条顺序断言变成同义反复。显式走
/// `search_with_weights`,而不是靠 `SearchIndex::search` 内部恰好也用默认值:后者
/// 是实现细节,前者是这份 fixture 的前提条件。权重**非**默认时排序会跟着变,由
/// 下面独立的 `non_default_weights_reorder_the_same_query` 钉住 —— 那条主张属于
/// 「权重真的在起作用」,不属于这份以默认权重为基准的回归集。
///
/// **它管不到 bm25。** 相关度本身有没有抵达 `score_of`,由 `query.rs` 的
/// `the_bm25_rank_column_actually_reaches_the_score` 单独钉住 —— 实测把 `rank` 的
/// 列索引读错,这里 69 条一条不红,只有那条单测会死(task 13 复现)。所以别把
/// 「回归集绿了」读成「排序没问题」。
///
/// 盲区的成因写在这里,免得下一个人白试一遍:60 条召回类断言的候选窗口
/// (`ORDER BY rank ASC LIMIT (limit*8).max(64)`)在 48 文件语料下从不截断,打分
/// 怎么错都不改变「有没有被召回进前 20」;9 条顺序断言的语料则是**刻意**构造成
/// bm25 打平的 —— 要证明档位乘数起作用,先消掉相关度这个混淆因子是前提。于是
/// 「补一条能抓住 bm25 的顺序用例」按定义就是一条 bm25 与档位乘数打架的用例,
/// 它同时就不再是一条干净的档位用例。同一条 fixture 记录说不了两件互斥的事。
#[test]
fn retrievability_regression_set_is_fully_recalled_and_correctly_ordered() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/retrievability.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!cases.is_empty(), "the regression set must not be empty");

    // One index per distinct `source_globs` value, built lazily and reused:
    // the default (no key at all = empty pattern set) plus whatever the
    // glob-bearing cases ask for.
    let mut indexes: Vec<(Vec<String>, (tempfile::TempDir, SearchIndex))> = Vec::new();

    let mut failures = Vec::new();
    for case in &cases {
        // Reject unknown keys (review round 1, Minor 2). Every optional key
        // in this schema is read with `case["k"]`, which yields
        // `Value::Null` — i.e. "not present" — for a key that is absent OR
        // misspelled. So a typo'd `expect_lines` silently stops asserting the
        // line anchor and the suite stays green while the case has quietly
        // become weaker than its `why` claims. That is the same class of
        // false-coverage bug this fixture exists to prevent, one level up, and
        // it matters more here than in most tests: this file is the branch's
        // only judge of ranking. Fail loudly on anything not in the schema.
        let known = [
            "query",
            "why",
            "source_globs",
            "expect_path",
            "expect_text",
            "expect_line",
            "expect_none",
            "expect_not_path",
            "outranks_path",
            "outranks_text",
        ];
        let Some(obj) = case.as_object() else {
            failures.push(format!("  {case} → every case must be a JSON object"));
            continue;
        };
        let unknown: Vec<&str> =
            obj.keys().map(String::as_str).filter(|k| !known.contains(k)).collect();
        if !unknown.is_empty() {
            failures.push(format!(
                "  {:?} → unknown fixture key(s) {unknown:?}; the schema is {known:?}. \
                 An unrecognized key is silently ignored, so a typo turns an assertion off \
                 without turning this suite red — which is why it is rejected here instead",
                case["query"].as_str().unwrap_or("(no query)"),
            ));
            continue;
        }
        let q = case["query"].as_str().unwrap();
        let globs: Vec<String> = case["source_globs"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
            .unwrap_or_default();
        if !indexes.iter().any(|(g, _)| *g == globs) {
            indexes.push((globs.clone(), open_temp_with_globs(&globs)));
        }
        let idx = &indexes.iter().find(|(g, _)| *g == globs).unwrap().1 .1;
        let answer = idx.search_with_weights(q, 20, &Limits::full(), &Weights::default()).unwrap();
        let (hits, route) = (answer.hits, answer.route);
        let seen = |hits: &[searchidx::Hit]| {
            hits.iter().take(3).map(|h| h.path.clone()).collect::<Vec<_>>().join(", ")
        };
        // A third, negative case shape alongside recall (`expect_path`) and
        // order (`outranks_path`): `origin:bogus` (task 6) must fail closed —
        // match nothing, not silently fall back to every tier — and that
        // claim can only be written as "zero hits", which `expect_path`'s
        // presence check cannot express. See `query.rs`'s
        // `an_unrecognized_origin_value_matches_nothing_not_everything` for
        // the pure-filter version of the same pin. task C-T12 reuses the same
        // shape for two more "zero hits IS the claim" cases that are not about
        // failing closed — a transcript outside every source glob must not be
        // indexed at all, and a file classified `derived` must not also answer
        // `origin:source` — so the message below states the fixture's own
        // reason (`why`) rather than assuming the filter-validation one.
        if case["expect_none"].as_bool() == Some(true) {
            if !hits.is_empty() {
                failures.push(format!(
                    "  {q:?} → expected no hits, got [{}] (route {}); the case's claim IS the emptiness — {}",
                    seen(&hits),
                    route.as_str(),
                    case["why"].as_str().unwrap_or("(no why recorded)"),
                ));
            }
            continue;
        }
        let want = case["expect_path"].as_str().unwrap();
        let want_text = case["expect_text"].as_str();
        let Some(want_at) = position_of(&hits, want, want_text) else {
            failures.push(format!(
                "  {q:?} → expected {want}{}, got [{}] (route {})",
                want_text.map(|t| format!(" containing {t:?}")).unwrap_or_default(),
                seen(&hits),
                route.as_str()
            ));
            continue;
        };
        // A fifth case shape (task C-T12): the transcript chunkers strip
        // sequence numbers and timecodes out of the indexed text but must
        // keep every block's line number pointing at the file's real text
        // line. Recall alone is blind to that — the words are in the block
        // either way; only the anchor the user clicks is wrong.
        if let Some(want_line) = case["expect_line"].as_u64() {
            let got = hits[want_at].line as u64;
            if got != want_line {
                failures.push(format!(
                    "  {q:?} → {want} must be anchored at line {want_line}, got line {got} \
                     (a timecode or sequence line, or an offset that forgot the body start)"
                ));
            }
        }
        // A fourth case shape (task 6 review round 1): `outranks_path` proves
        // ordering, but a filter case (`origin:human`) needs to prove
        // EXCLUSION, which no positive assertion here can express — a
        // presence check on `expect_path` alone stays green even with the
        // filter completely disabled, because the unfiltered query still
        // recalls the same file (verified empirically: disabling
        // `push_filters`'s origin clause left the three `tieringtoken
        // origin:<tier>` cases green while only `expect_none` caught it).
        // `expect_not_path` closes that gap: the named path must be ABSENT
        // from the (up to `limit`) hits actually returned.
        if let Some(not_want) = case["expect_not_path"].as_str() {
            if hits.iter().any(|h| h.path == not_want) {
                failures.push(format!(
                    "  {q:?} → expected {not_want} to be filtered out, but it was still present: got [{}]",
                    seen(&hits)
                ));
            }
        }
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

/// task C-T12 的第六条新用例:**权重非默认时排序随之改变**。
///
/// 这条刻意留在 `retrievability.json` 之外。那份 fixture 的前提条件是
/// 「排序裁判 = `Weights::default()`」——把一条依赖用户自定义权重的用例塞进去,
/// 等于在裁判席上再放一个裁判。这里要证的也不是「某份文档应该排在前面」(那是
/// fixture 的事),而是「`Weights` 这个旋钮真的接在排序上」:C-T7 之前四档乘数
/// 是 `score_of` 里的字面量,一个把 `weights` 参数收下却继续用字面量的实现,
/// 设置页会照常保存、照常显示,搜索结果却纹丝不动 —— GUI 侧测不出来,只有
/// 端到端跑一次两种权重才看得见。
///
/// 用 `tieringtoken` 这一组语料(六份正文词数一致、doc_date 相同、raw bm25 精确
/// 相等的文件,见 fixture 里那条 why),两端各取一极:默认权重下 human ×1.25 的
/// `notes/…tiering-note.md` 在最前、unlabeled ×0.3 的 `sync/…tiering-mirror.md`
/// 在最后;把 `unlabeled` 调到 5.0(`Weights::sanitized` 允许的上限),同一个
/// 索引、同一条查询,两端必须**对调**。断言的是完整的方向翻转,不只是「分数变
/// 了」——后者一个把权重加进分数却不参与排序的实现也能满足。
#[test]
fn non_default_weights_reorder_the_same_query() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();

    let paths = |w: &Weights| {
        idx.search_with_weights("tieringtoken", 20, &Limits::full(), w)
            .unwrap()
            .hits
            .iter()
            .map(|h| h.path.clone())
            .collect::<Vec<_>>()
    };

    let human = "notes/2026-08-05-tiering-note.md";
    let unlabeled = "sync/2026-08-05-tiering-mirror.md";

    let shipped = paths(&Weights::default());
    assert_eq!(shipped.first().map(String::as_str), Some(human), "{shipped:?}");
    assert_eq!(shipped.last().map(String::as_str), Some(unlabeled), "{shipped:?}");

    // Only `unlabeled` moves; every other tier keeps its shipped value, so
    // nothing but this one knob can explain the flip.
    let tuned = paths(&Weights { unlabeled: 5.0, ..Weights::default() });
    assert_eq!(
        tuned.first().map(String::as_str),
        Some(unlabeled),
        "a user-raised `unlabeled` weight must actually reorder retrieval, not just be stored: {tuned:?}"
    );
    assert!(
        tuned.iter().position(|p| p == human).unwrap() > 0,
        "and the previously-first human hit must have been pushed down: {tuned:?}"
    );
    let mut shipped_set = shipped.clone();
    let mut tuned_set = tuned.clone();
    shipped_set.sort();
    tuned_set.sort();
    assert_eq!(
        shipped_set, tuned_set,
        "weights re-rank; they must not change WHICH files are recalled: {shipped:?} vs {tuned:?}"
    );
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

// --- wikipage 检索优先级 ------------------------------------------------
// spec `docs/superpowers/specs/2026-08-12-wikipage-search-priority-design.md`

/// §2/§3:通过 wikilink 建出来的页面,正文就是一个空节点(`- `),标题只躺在
/// front-matter 里,而文件名是 slug 化的原文。在标题进 FTS 之前,搜这个页
/// 的名字命中不了它 —— 不是排序低,是这条结果根本不存在。
///
/// 两个查询都要绿:`title`(fm 原文)和文件名 stem 是两份可能不同的数据,
/// 而 wikilink 在本产品里是**按文件名解析**的,所以两个都必须可搜。
#[test]
fn a_files_title_and_filename_are_searchable_when_the_body_never_says_them() {
    let v = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(v.path().join("wikipage")).unwrap();
    std::fs::write(v.path().join("wikipage/zhang-san.md"), "---\ntitle: 张三\n---\n- \n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    for query in ["张三", "zhang-san"] {
        let (hits, _) = idx.search(query, 20).unwrap();
        assert!(
            hits.iter().any(|h| h.path == "wikipage/zhang-san.md"),
            "查询 {query:?} 找不到它自己的页面: {hits:?}"
        );
    }
}

/// §3:标题只进 `blocks_fts.tok_title`,不进 `blocks.text`。这两份数据本来
/// 就是分离的,但「把标题拼进 File 块的 text」是这个功能最省事的实现路线 ——
/// 走了那条路,命中预览里会凭空多出标题,而这条测试是唯一会红的地方。
#[test]
fn a_title_match_never_leaks_the_title_into_the_hit_text() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("page.md"), "---\ntitle: 张三\n---\n只有正文没有名字\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    let (hits, _) = idx.search("张三", 20).unwrap();
    let hit = hits.iter().find(|h| h.path == "page.md").expect("标题命中不见了");
    assert!(!hit.text.contains("张三"), "标题漏进了展示文本: {:?}", hit.text);
}

/// §3:`tok_title` 只写在 File 级块上。写在每个块上(另一条省事路线)会让
/// 「搜文件名 → 这个文件的每一段都命中」,同一份证据按块数重复一遍。
#[test]
fn only_the_file_level_block_can_match_on_a_title() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(
        v.path().join("page.md"),
        "---\ntitle: 张三\n---\n# 小节甲\n\n第一段内容\n\n# 小节乙\n\n第二段内容\n",
    )
    .unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    let (hits, _) = idx.search("张三", 50).unwrap();
    let mine: Vec<_> = hits.iter().filter(|h| h.path == "page.md").collect();
    assert_eq!(mine.len(), 1, "标题命中应当只有一条(File 级): {mine:?}");
    assert_eq!(mine[0].level, "file");
}

/// §5 端到端:`score_of` 的 ×1.5 是纯函数测试钉住的,但「`finish` 真的从
/// `hit.text` 算出了这个 flag」只有跑一遍真索引才验得到 —— 把那里写死成
/// `false`,上面那条纯函数测试照样绿。
///
/// 这对 fixture 的 tok_text **逐 token 相同**(`[[…]]` 的方括号不是词字符,
/// 分词后两边都只剩 `mentiontoken`),所以 bm25 完全打平,唯一的差别只能是
/// 这一档加权 —— 没有长度归一化的混杂因素。
#[test]
fn a_linked_mention_outranks_the_same_words_written_plainly() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("alpha.md"), "steady content 见 mentiontoken 完\n").unwrap();
    std::fs::write(v.path().join("bravo.md"), "steady content 见 [[mentiontoken]] 完\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    let (hits, _) = idx.search("mentiontoken", 20).unwrap();
    let linked = hits.iter().position(|h| h.path == "bravo.md").expect("链接那条不见了");
    let plain = hits.iter().position(|h| h.path == "alpha.md").expect("裸写那条不见了");
    assert!(linked < plain, "[[提及]] 必须排在同样字面的裸文本之前: {hits:?}");
}

/// 置顶用例共用的 vault:一个 wikipage,和一个**逐字同形、只多了一条
/// `verified: by: human:` 的诱饵**放在 vault 根上。
///
/// 诱饵为什么要长成这样,试错记录如下(这段是给后来改 fixture 的人看的):
/// 先用「一篇反复念叨同一个词的长文」当诱饵 —— 输了,因为标题现在有自己的
/// FTS 列且权重 4.0(§3),而 bm25 偏爱短文档,一个空页在名字命中上稳赢任何
/// 长文。再把诱饵也改成精确同名 —— 还是输,同样的道理。
///
/// 所以诱饵改用**排序信号**而不是内容取胜:两个文件的块逐字相同,唯一差别
/// 是 `human_verified`(×1.1)与它带来的 origin 档位。诱饵因此稳定地排在
/// wikipage 前面,而置顶是唯一能把名次翻回来的力量 —— 这正是 §4 那句
/// 「哪怕另一篇 bm25 高得多、或 origin 是 human」要钉的东西。
///
/// 目录名由调用方决定,好让「改目录名」那条用例复用。
fn pin_vault(page_dir: &str) -> (tempfile::TempDir, tempfile::TempDir, SearchIndex) {
    let v = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(v.path().join(page_dir)).unwrap();
    std::fs::write(v.path().join(page_dir).join("张三.md"), "---\ntitle: 张三\n---\n- \n").unwrap();
    std::fs::write(
        v.path().join("张三.md"),
        "---\ntitle: 张三\nverified:\n  by: human:bruce\n---\n- \n",
    )
    .unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();
    (v, d, idx)
}

fn conv(dir: &str) -> Conventions {
    Conventions { wikipage_dir: Some(dir.to_string()) }
}

/// §4:名字与查询完全相同的 wikipage 硬置顶为第一条,绕过所有加权。
///
/// 前半段先证明这个 fixture 是有效的 —— 不给 `Conventions` 时那篇长文确实
/// 排在前面。少了这一步,「置顶生效」就可能只是「它本来就是第一」。
#[test]
fn a_wikipage_named_exactly_like_the_query_is_pinned_to_the_top() {
    let (_v, _d, idx) = pin_vault("wikipage");
    let w = Weights::default();

    let baseline = idx.search_with_weights("张三", 20, &Limits::full(), &w).unwrap();
    assert_ne!(
        baseline.hits[0].path, "wikipage/张三.md",
        "fixture 失效:没有置顶时它就已经是第一条了,这条用例证明不了任何事"
    );

    let a = idx.search_ranked("张三", 20, &Limits::full(), &w, &conv("wikipage")).unwrap();
    assert_eq!(a.hits[0].path, "wikipage/张三.md", "精确同名的 wikipage 必须置顶: {:?}", a.hits);
    assert!(a.hits[0].pinned);
}

/// §4:多词查询谈不上「这个关键词的页」。
#[test]
fn a_multi_term_query_does_not_pin() {
    let (_v, _d, idx) = pin_vault("wikipage");
    let a = idx
        .search_ranked("张三 张三", 20, &Limits::full(), &Weights::default(), &conv("wikipage"))
        .unwrap();
    assert!(a.hits.iter().all(|h| !h.pinned), "多词查询不该置顶: {:?}", a.hits);
}

/// §4:带过滤器时用户是在做精确检索,不该被一条置顶插队。
#[test]
fn a_filtered_query_does_not_pin() {
    let (_v, _d, idx) = pin_vault("wikipage");
    let a = idx
        .search_ranked("张三 ext:md", 20, &Limits::full(), &Weights::default(), &conv("wikipage"))
        .unwrap();
    assert!(a.hits.iter().all(|h| !h.pinned), "带过滤器不该置顶: {:?}", a.hits);
}

/// §4:置顶是 wikilink 目录的特权,vault 里别处的同名文件不享受。
#[test]
fn a_same_named_file_outside_the_wikipage_dir_is_not_pinned() {
    let (_v, _d, idx) = pin_vault("wikipage");
    let a = idx
        .search_ranked("张三", 20, &Limits::full(), &Weights::default(), &conv("别的目录"))
        .unwrap();
    assert!(
        a.hits.iter().all(|h| !h.pinned),
        "配置指向别的目录时,wikipage/ 下的同名文件不该被置顶: {:?}",
        a.hits
    );
}

/// §1 的硬要求:目录名是用户随时可改的配置,改完必须立刻生效,**不重建索引**。
///
/// 这条是整个设计里「wikipageDir 走查询侧传参、不进索引」这个决定的钉子:
/// 一旦有人把目录名塞进索引(存成列、或写进 meta 戳),同一个 `idx` 换个
/// `Conventions` 就不会改变结果,这条立刻红。
#[test]
fn renaming_the_wikipage_dir_takes_effect_without_reindexing() {
    let (_v, _d, idx) = pin_vault("概念");
    let w = Weights::default();

    let old = idx.search_ranked("张三", 20, &Limits::full(), &w, &conv("wikipage")).unwrap();
    assert!(old.hits.iter().all(|h| !h.pinned), "旧目录名不该再置顶: {:?}", old.hits);

    let new = idx.search_ranked("张三", 20, &Limits::full(), &w, &conv("概念")).unwrap();
    assert_eq!(new.hits[0].path, "概念/张三.md", "改名后必须立刻置顶: {:?}", new.hits);
    assert!(new.hits[0].pinned);
}

/// §4:文件名 slug 化、fm `title` 存原文,是 wikilink 建页的常态
/// (`src/lib/outline/create.ts`)。只按文件名判定的话,这类页面永远置不了顶。
#[test]
fn a_wikipage_whose_title_matches_is_pinned_even_when_its_filename_is_slugged() {
    let v = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(v.path().join("wikipage")).unwrap();
    std::fs::write(v.path().join("wikipage/zhang-san.md"), "---\ntitle: 张三\n---\n- \n").unwrap();
    std::fs::write(
        v.path().join("张三.md"),
        "---\ntitle: 张三\nverified:\n  by: human:bruce\n---\n- \n",
    )
    .unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();
    let w = Weights::default();

    let baseline = idx.search_with_weights("张三", 20, &Limits::full(), &w).unwrap();
    assert_ne!(
        baseline.hits[0].path, "wikipage/zhang-san.md",
        "fixture 失效:没有置顶时它就已经是第一条了"
    );

    let a = idx.search_ranked("张三", 20, &Limits::full(), &w, &conv("wikipage")).unwrap();
    assert_eq!(a.hits[0].path, "wikipage/zhang-san.md", "fm title 匹配也要置顶: {:?}", a.hits);
}

// --- 索引体积 ----------------------------------------------------------
// 实测(2026-08-13,8,977 文件的真实 vault):index.db 1.6 GB,而 index.db-wal
// 另有 1.7 GB —— 磁盘占用的一半是重建期涨起来、之后再没缩回去的 WAL 高水位。
// WAL 只会被复用,不会自己变小,除非做一次 TRUNCATE 检查点。

/// 一次全量重建之后,WAL 必须被还给磁盘,而不是留着重建期的高水位。
#[test]
fn a_full_rebuild_truncates_the_write_ahead_log() {
    let v = tempfile::tempdir().unwrap();
    // 要足够多的内容把 WAL 顶起来 —— 太小的语料在默认 autocheckpoint
    // (1000 页)以内,测试会因为「本来就没涨」而假绿。
    for i in 0..400 {
        std::fs::write(
            v.path().join(format!("f{i}.md")),
            format!("# 标题 {i}\n\n{}\n", "内容 content alpha bravo charlie ".repeat(60)),
        )
        .unwrap();
    }
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("i.db");
    let mut idx = SearchIndex::open_at(v.path(), &db, "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    let wal = db.with_file_name("i.db-wal");
    let wal_bytes = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
    let db_bytes = std::fs::metadata(&db).unwrap().len();
    assert!(
        db_bytes > 1_000_000,
        "语料太小,WAL 根本没机会涨起来,这条测试证明不了任何事(db={db_bytes})"
    );
    assert!(
        wal_bytes < 100_000,
        "重建后 WAL 没有被截断:{wal_bytes} 字节(db={db_bytes})"
    );
}

/// 实测(同一次审计):`blocks_fts_content` 632 MB,占 1.6 GB 索引的 39% ——
/// FTS5 默认会把分词后的文本**再存一份**在 `%_content` 表里。
///
/// 这一份从来没人读:查询一律 JOIN 回 `blocks` 取真正的文本,没有任何
/// `snippet()`/`highlight()` 调用,`bm25()` 在 contentless 表上照常工作。
/// 所以它是纯粹的浪费,而这条测试就是那 39% 的钉子 —— 谁把 `content=''`
/// 从建表语句里去掉(比如一次粗心的合并),这里立刻红。
#[test]
fn the_fts_table_does_not_keep_a_second_copy_of_the_tokenized_text() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("a.md"), "alpha bravo 增量索引\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("i.db");
    let mut idx = SearchIndex::open_at(v.path(), &db, "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();
    drop(idx);

    let conn = rusqlite::Connection::open(&db).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !tables.iter().any(|t| t == "blocks_fts_content"),
        "FTS 又存了一份分词文本(索引体积的 39%): {tables:?}"
    );
}

/// contentless 的唯一行为差异,写下来免得将来有人踩:读 FTS 的列值不会报错,
/// 会**静默返回 NULL**。今天没有任何代码这么做(查询取的是 `b.text`),
/// 但如果哪天有人顺手写了 `SELECT tok_text FROM blocks_fts`,他不会看到
/// 一个错误,只会看到空 —— 这条测试把这个陷阱记在案。
#[test]
fn reading_an_fts_column_value_yields_null_rather_than_an_error() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("a.md"), "alpha bravo\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("i.db");
    let mut idx = SearchIndex::open_at(v.path(), &db, "sync").unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();
    drop(idx);

    let conn = rusqlite::Connection::open(&db).unwrap();
    let got: Option<String> = conn
        .query_row("SELECT tok_text FROM blocks_fts WHERE blocks_fts MATCH '\"alpha\"' LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(got, None, "contentless 表不该还留着列值");
}

/// 摄取端到端:vault 里放一份 analytics 文件,refresh 后表里就有对应的行。
/// 用真实临时目录而不是内存库 —— 这条要验的正是「读盘 → 折算 → 落表」这条链
/// 有没有接错,而不是三段各自的算术(那些在 attention.rs 的单测里)。
#[test]
fn refresh_attention_ingests_analytics_into_the_index() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "# 标题\n正文\n").unwrap();
    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","day":"{today}","docs":{{"rel:a.md":{{"read_ms":600000,"edit_ms":0,"open_count":1,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}"#
        ),
    )
    .unwrap();

    // `open_temp` 返回 `(TempDir, SearchIndex)`,那个 TempDir 装的是 index.db ——
    // 必须绑住,提前 drop 会把库删掉。
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();
    assert_eq!(idx.refresh_attention(&[]).unwrap(), 1);
    let stats = idx.stats().unwrap();
    assert_eq!(stats.attention_files, 1);

    // 重复调用是幂等的 —— 全量重算的核心保证,也是「不做增量」的理由。
    assert_eq!(idx.refresh_attention(&[]).unwrap(), 1);
    assert_eq!(idx.stats().unwrap().attention_files, 1);
}

/// 没有 analytics 目录 = 从没开过洞察 = 空表,不是错误 —— 但「跑过、零结果」
/// 与「从没跑过」必须是两个可区分的状态,不能都读成 `None`。
#[test]
fn refresh_attention_on_a_vault_without_insights_is_a_clean_no_op() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "正文\n").unwrap();
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();

    // 摄取真的从没跑过:`attention_as_of` 必须是 `None`。这一侧是区分力的
    // 前半段 —— 少了它,下面「跑过之后变成 Some」的断言测不出任何东西,因为
    // 一个恒等于 `Some` 的错误实现也会让它通过。
    assert_eq!(idx.stats().unwrap().attention_as_of, None, "调用前必须是 None");

    assert_eq!(idx.refresh_attention(&[]).unwrap(), 0);
    let stats = idx.stats().unwrap();
    assert_eq!(stats.attention_files, 0, "没有 analytics 目录,零结果");
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    assert_eq!(
        stats.attention_as_of,
        Some(today),
        "跑过一轮之后即使零结果,attention_as_of 也必须变成 Some(今天) —— \
         这是与「从没跑过」区分的唯一途径"
    );
}

/// 80 篇噪声 + 1 篇 bm25 严格垫底、但你在上面花了 10 小时的 `target.md`,外加
/// 对应的 analytics 日文件。两条测试共用:一条验「默认 k 下它被捞回来」,一条
/// 验「k=0 下整条链路与没有注意力数据时逐条相同」。抽成函数而不是抄两份 ——
/// 抄件一旦漂移,两条测试就在各自不同的语料上各说各话,而它们的意义恰恰来自
/// 「同一份 fixture,只有 k(和 `target_frontmatter`)不同」。
///
/// `target_frontmatter` 决定目标文档的 origin 档位,而这**不是可有可无的调味
/// 料**:噪声一律无 frontmatter(rule 6′ → `Unlabeled`,×0.3)。传 `""` 时目标
/// 也是 `Unlabeled`,与噪声同档,于是它即使被保底臂捞进评分也仍排在噪声之后、
/// 被 `truncate(limit)` 切掉 —— 只有注意力加成能把它顶上来。传
/// `"---\ntype: Note\n---\n"` 时目标是 `Human`(×1.25),这时**即使 k=0**、
/// 加成恒为 ×1.0,只要保底臂把它捞进评分它就会挤掉窗口内的噪声。后者正是最终
/// 评审 I-1 的失败场景,`k_zero_keeps_the_whole_pipeline_identical_to_having_
/// no_attention_data` 用的就是它:换成 `""` 那条测试会在 bug 存在时照样绿。
fn vault_with_one_high_attention_document(target_frontmatter: &str) -> tempfile::TempDir {
    let vault = tempfile::tempdir().unwrap();
    // 噪声:80 篇短文,每篇都命中查询词,足以填满 (limit*8).max(64) 的窗口
    // (limit=10 → 80 条,而每篇噪声还各自贡献一条 File 级 rollup,所以窗口
    // 其实被噪声塞得满满当当)。
    for i in 0..80 {
        std::fs::write(vault.path().join(format!("noise{i}.md")), "银河 的 观测 记录\n").unwrap();
    }
    // 目标:同样只命中一次,但比噪声更长 —— bm25 的长度归一化让它严格垫底,
    // 排在候选窗口之外。
    std::fs::write(
        vault.path().join("target.md"),
        format!("{target_frontmatter}银河 的 观测 记录 补记\n"),
    )
    .unwrap();

    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","day":"{today}","docs":{{"rel:target.md":{{"read_ms":36000000,"edit_ms":0,"open_count":9,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}"#
        ),
    )
    .unwrap();
    vault
}

/// 规格 §5:`fts_search` 先按 bm25 `ORDER BY rank ASC LIMIT (limit*8).max(64)`
/// 取候选、**再**跑 `score_of`,所以纯排序加权救不了排在窗口外的文档 —— 它在
/// 打分之前就被砍掉了,再大的系数也没机会作用在它身上。第二条臂给这类文档一
/// 个进入评分阶段的名额。这里构造:80 篇噪声把 `limit=10` 的 80 条候选窗口填
/// 满,外加一份 bm25 严格垫底、但你在上面花了 10 小时的文档。
///
/// **fixture 与简报原稿不同,差异是量算出来的,不是口味。** 简报原稿用的是
/// 「500 行填充 + 查询词只出现一次」的长文:实测它的 `score_of` 结果比噪声低
/// **约 45 倍**(噪声 7.41e-7 vs 长文 1.65e-8),而注意力加成的**天花板**是
/// `1 + k`,默认 k=0.4 即 ×1.4(`attention::boost`,`frac` 已封顶 1.0)。所以
/// 那份 fixture 在**任何**候选阶段的改动下都进不了前 10 —— 它能被捞进评分,
/// 但评分之后仍排在 80 篇噪声之后,被最终的 `truncate(limit)` 切掉。要让它可
/// 见只能给注意力臂在**输出**里留保留席位,而规格 §5 明确拒绝了这条路(「主
/// 排序仍然是相关度」)。所以这里把 bm25 差距收窄到加成天花板之内(实测比值
/// ≈1.10,余量 27%),让这条测试断言它**能**断言的那件事:窗口外 = 不可见,
/// 有注意力 = 进候选 = 可见。差距本身与本测试无关 —— 排序算术归
/// `query.rs` 的 `attention_alone_moves_the_score` 等单测管。
#[test]
fn a_high_attention_document_is_recalled_past_the_bm25_window() {
    // 目标与噪声同档(都无 frontmatter → `Unlabeled`):这条测试要证明的是
    // **注意力**把它顶了上来,所以不能让 origin 差异替它干活。
    let vault = vault_with_one_high_attention_document("");
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();

    // 摄取之前:没有注意力数据,它进不了候选。
    let before = idx.search("银河", 10).unwrap().0;
    assert!(
        !before.iter().any(|h| h.path == "target.md"),
        "基线错了:它本就该在纯 bm25 下落榜,否则这条测试证明不了任何事"
    );
    // 而且落榜的原因必须是**窗口**,不是「压根没 MATCH 上」—— 后者会让上面
    // 那条断言在一个把查询词拼错的 fixture 上也「通过」,于是整条测试变成
    // 空转。放大 limit(窗口随之放大到 1600)它就该出现。
    let wide = idx.search("银河", 200).unwrap().0;
    assert!(
        wide.iter().any(|h| h.path == "target.md"),
        "它必须是命中的,只是 bm25 垫底:{:?}",
        wide.len()
    );

    idx.refresh_attention(&[]).unwrap();
    let after = idx.search("银河", 10).unwrap().0;
    assert!(
        after.iter().any(|h| h.path == "target.md"),
        "注意力臂必须把它捞回来:{:?}",
        after.iter().map(|h| &h.path).collect::<Vec<_>>()
    );
}

/// 规格 §4.2/§7:`k = 0` 是**回滚开关** ——「全链路输出与接入前逐位相同」。
///
/// 这条与 `query.rs` 里那两条(`k_zero_disables_the_boost_in_score_of`、
/// `zero_attention_leaves_the_score_bit_identical`)不是重复:那两条钉的是
/// `score_of` 这一个函数的算术,而最终评审 I-1 实测到的失效**绕过了它们**
/// —— 打分确实没变,变的是**候选集**。保底臂捞回来的行照样过 `finish`,
/// origin 四档乘数、`agent_by`、`doc_date` 加成对它们照常作用,于是一份
/// 「bm25 在窗口外但别的加成站在它这边」的文档能挤掉窗口内的对手,而它能
/// 进入评分阶段这件事只由注意力臂促成。所以这条比的是**最终可见的 hit
/// 序列**,而不是任何单个函数的返回值。
///
/// 两条断言缺一不可:
/// - `k=0`:摄取前后的 `(path, line, score)` 序列必须逐条相同;
/// - `k=默认`(对照臂):同一份 fixture 必须**变**。少了它,一个「注意力
///   压根不起作用」的实现(或一份选错的 fixture)也能让上面那条通过,整条
///   测试空转 —— 这正是 I-1 能躲过 13 轮逐环评审的原因。
#[test]
fn k_zero_keeps_the_whole_pipeline_identical_to_having_no_attention_data() {
    // 目标是 `Human`(×1.25)、噪声是 `Unlabeled`(×0.3) —— 见 fixture 的注释:
    // 这正是「候选集变了 → 可见结果就变了,哪怕加成恒为 ×1.0」的那一类文档。
    let vault = vault_with_one_high_attention_document("---\ntype: Note\n---\n");
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();

    let off = Weights { attention: 0.0, ..Weights::default() };
    // 比 `(path, line, score)` 而不是整个 `Hit`:`Hit::attention_minutes` 是
    // 纯展示字段(CLI `--json` 用它解释顺序),摄取后它当然会从 0 变成 600,
    // 那不是「排序被改了」。k=0 要保证的是**排序与可见集**不变。
    let shape = |hits: &[Hit]| {
        hits.iter().map(|h| (h.path.clone(), h.line, h.score)).collect::<Vec<_>>()
    };
    let run = |idx: &SearchIndex, w: &Weights| {
        idx.search_with_weights("银河", 10, &Limits::full(), w).unwrap().hits
    };

    let before_off = run(&idx, &off);
    let before_on = run(&idx, &Weights::default());
    idx.refresh_attention(&[]).unwrap();
    let after_off = run(&idx, &off);
    let after_on = run(&idx, &Weights::default());

    assert_eq!(
        shape(&before_off),
        shape(&after_off),
        "k=0 是回滚开关:注意力数据的存在不许改变任何一位输出 —— 打分关了还不够,\
         候选臂也必须一起关"
    );
    // 对照臂:证明这份 fixture 真的有区分力。
    assert!(
        !before_on.iter().any(|h| h.path == "target.md"),
        "基线错了:摄取前它本就该落榜"
    );
    assert!(
        after_on.iter().any(|h| h.path == "target.md"),
        "默认 k 下注意力必须改变结果,否则上面那条「没变」的断言什么都没证明:{:?}",
        after_on.iter().map(|h| &h.path).collect::<Vec<_>>()
    );
}

/// 第二条臂共用同一个 MATCH 条件,所以**不能**引入不匹配的结果 ——
/// 「我读得最多的文档」不是「我搜的东西」。
#[test]
fn the_attention_arm_never_introduces_a_non_matching_hit() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("read-a-lot.md"), "完全无关的内容\n").unwrap();
    std::fs::write(vault.path().join("match.md"), "银河\n").unwrap();
    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","day":"{today}","docs":{{"rel:read-a-lot.md":{{"read_ms":36000000,"edit_ms":0,"open_count":9,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}"#
        ),
    )
    .unwrap();

    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();
    idx.refresh_attention(&[]).unwrap();
    let hits = idx.search("银河", 10).unwrap().0;
    assert!(hits.iter().all(|h| h.path != "read-a-lot.md"), "注意力不是匹配条件");
    assert!(hits.iter().any(|h| h.path == "match.md"));
}
