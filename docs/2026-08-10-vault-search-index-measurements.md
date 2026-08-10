# Vault search index — measured footprint (task 19)

Companion to `docs/2026-08-10-vault-search-index-design.md` §3.1/§7. That
document's "体积决策" section recorded a **planning-phase estimate** from a
standalone probe binary (sqlite ≈+0.90 MB, jieba default-dict ≈+3.60 MB,
pulldown-cmark ≈+0.18 MB, total ≈+4.74 MB), which pushed the budget from the
spec's original <4 MB to a revised **<5.0 MB**. This document replaces that
estimate with a measurement of the actual application binary at two real
commits. All figures below were produced by the exact commands shown, on this
machine (macOS, Apple Silicon), on 2026-08-11. Nothing here is inferred or
rounded from a source other than what is shown.

## 1 · Binary size — baseline vs. current

- **Baseline** = release build at `git merge-base main HEAD` = `f741b4d7067c`
  ("feat(website): 网站认 Windows..." — main's tip before this branch;
  confirmed by inspection that `src-tauri/Cargo.toml` at this commit has no
  `searchidx`/`rusqlite`/`jieba` dependency).
- **Current** = release build at `HEAD` = `02c7759df694`.
- Built without disturbing this worktree: baseline was built in a separate
  temporary `git worktree add --detach /tmp/notemd-baseline-wt f741b4d`,
  measured, then removed (`git worktree remove /tmp/notemd-baseline-wt`).
- Both commits share the identical `[profile.release]` in `src-tauri/Cargo.toml`
  (`opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`,
  `strip = true`) — verified with `git show f741b4d:src-tauri/Cargo.toml` vs.
  the working tree, so the comparison is apples-to-apples.
- `dist/` (the frontend build) was absent for both builds; the release binary
  compiled successfully either way, confirming the Tauri build does not embed
  frontend assets into the compiled Mach-O binary for a bare
  `cargo build --release` (frontend assets are bundled into `Contents/Resources`
  only by the full `tauri build`/bundling step, not the native compile) — so
  this binary-size comparison isolates the native/Rust delta from any frontend
  growth (e.g. the new search side panel's JS/CSS).

Commands:

```bash
# baseline
git worktree add --detach /tmp/notemd-baseline-wt f741b4d
cd /tmp/notemd-baseline-wt
cargo build --manifest-path src-tauri/Cargo.toml --release
stat -f%z src-tauri/target/release/notemd
# → 8547392

# current (this worktree, at HEAD)
cargo build --manifest-path src-tauri/Cargo.toml --release
stat -f%z src-tauri/target/release/notemd
# → 12644000

git worktree remove /tmp/notemd-baseline-wt
```

| | bytes | MiB (1024-based) | MB (decimal) |
| --- | ---: | ---: | ---: |
| Baseline (`f741b4d`) | 8,547,392 | 8.15 | 8.55 |
| Current (`HEAD` / `02c7759`) | 12,644,000 | 12.06 | 12.64 |
| **Delta** | **4,096,608** | **3.91** | **4.10** |

**Verdict: the delta is ≈4.1 MB (decimal) / ≈3.9 MiB — under the revised 5.0 MB
budget**, and (using the MiB convention) also under the spec's original 4 MB
estimate. It is *lower* than the planning-phase probe's ≈4.74 MB sum. A
plausible reason, not independently re-verified here: the probe measured
`rusqlite`(bundled)+`jieba-rs`+`pulldown-cmark` in isolation, while in the
whole app some of their transitive dependencies (e.g. compression, hashing,
random-number infrastructure) were already linked in for other features, so
whole-program LTO could eliminate more duplicate code than the standalone
probe's smaller crate graph allowed. **This document does not attempt to
re-derive a fresh per-component breakdown of the real delta** (sqlite alone /
+jieba / +pulldown-cmark) — doing so honestly would require building three
more full-LTO intermediate binaries, and the only component breakdown
available is the planning-phase probe's, which is cited above with that
attribution and should not be read as remeasured against the real app.

### What this does NOT measure

- **Installed size (`.app` bundle)** and **download size (compressed `.dmg`)**
  depend on codesigning, the packaged frontend (`dist/`), plugin bundles, and
  DMG compression — none of which this task built (no code-signing identity
  available in this environment, and building the full release pipeline is
  out of scope for a footprint measurement). **Not measured.**
- The README's "~11 MB download / ~15 MB installed" figures (§4 below) are
  therefore an **estimate obtained by adding the measured bare-binary delta
  (+4.1 MB decimal, rounded) to the previous README figures (7 MB / 11 MB)**,
  under the explicit assumption that the download-compression ratio and the
  bundle's non-binary overhead (icons, plugin payloads, frontend assets)
  scale roughly 1:1 with the native binary's growth. That assumption is
  unverified — the true numbers could differ if, say, DMG compression handles
  the added `rusqlite`/`jieba` machine code less well than the rest of the
  binary. If precise download/install figures are needed, they require an
  actual signed `release.sh` run on both commits.

## 2 · Index size vs. corpus size

Fixture corpus: `searchidx/tests/fixtures/corpus` — **36 `.md`/`.note.md`
files, 144 KB on disk** (`du -sh`). This is the acceptance suite's synthetic
fixture set, not the design spec's 10k-file/150 MB real-vault anchor —
labeled accordingly throughout this section.

```bash
CORPUS="$(pwd)/searchidx/tests/fixtures/corpus"
./src-tauri/target/release/notemd --cli search --vault "$CORPUS" --rebuild --stats --json
# → {"blocks":97,"built_at":"...","db_bytes":4096,"files":36,"tokenizer_id":"v1+jieba-rs-0.10+cut_for_search+hmm"}
# (db_bytes is read mid-process, before SQLite's WAL checkpoints back to the
# main file on connection close — see file size below for the real on-disk
# figure)
ls -la "$HOME/Library/Application Support/net.notemd.app/search/2afa1853f5427d62/index.db"
# → 102400 bytes
```

- Corpus: 144 KB (36 files, 97 indexed blocks)
- Index on disk after checkpoint: **102,400 bytes (100 KB)**
- Ratio: index ≈ 0.71× corpus size at this scale.

This ratio is **not** a reliable predictor for a real vault. At 36 tiny files,
SQLite/FTS5's fixed per-database overhead (schema pages, FTS5 auxiliary
tables' minimum page allocations) dominates; at the spec's 10k-file anchor the
same fixed overhead amortizes over far more content and the ratio would be
expected to fall, but this was not measured — no 10k-file corpus was built or
run against.

## 3 · Cold build time

CLI process wall time (`--rebuild`, includes process startup and the CLI's
one-time jieba dictionary decompression — the corpus contains CJK content, so
the ~200–400 ms lazy load described in the design spec §3.2 is paid):

```bash
CORPUS="$(pwd)/searchidx/tests/fixtures/corpus"
rm -f "$HOME/Library/Application Support/net.notemd.app/search/2afa1853f5427d62/index.db"*
/usr/bin/time -p ./src-tauri/target/release/notemd --cli search --vault "$CORPUS" --rebuild --stats --json
```

Three runs (fresh index each time): **real 0.17s, 0.15s, 0.16s** — i.e.
≈150–170 ms end to end for a 36-file/144 KB corpus, dominated by the jieba
dictionary decompression, not the scan itself. Labeled: this is a CLI-process
number on a tiny synthetic corpus, not a scan-only number and not evidence
about a 10k-file vault.

## 4 · Unchanged-sweep time

```bash
/usr/bin/time -p ./src-tauri/target/release/notemd --cli search --vault "$CORPUS" --stats --json
```

Three runs against the already-built index: **real 0.01s, 0.00s, 0.00s** —
i.e. under `/usr/bin/time`'s 10 ms display resolution; the sweep is a
stat-fast-path no-op (`files_indexed == 0`). The crate's own
`an_unchanged_sweep_is_fast` acceptance test (in-process, no CLI startup cost)
asserts this stays under 300 ms and passes — see §6.

## 5 · Query p50 / p95

Measured in-process (no CLI startup cost), release build, 200 warm iterations
of `idx.search("search", 20)` after a 5-iteration warmup, over the same
36-file corpus. Produced with a temporary throwaway test file
(`searchidx/tests/zz_temp_timing.rs`, deleted immediately after — not part of
the committed suite):

```
p50 = 17 µs
p95 = 18 µs
max = 18 µs   (n = 200)
```

Both are far under the spec §7 budget (p50 < 10 ms warm). Labeled: this is a
36-file synthetic corpus in a warm process; it demonstrates the FTS path is
taken (not a full scan) and is fast, not a performance curve for a 10k-file
vault.

## 6 · Acceptance suite (spec §7 as tests)

```bash
cargo test --manifest-path searchidx/Cargo.toml --release --test acceptance -- --nocapture
```

```
running 9 tests
test reindexing_one_file_is_well_under_the_freshness_budget ... ok
test an_unchanged_sweep_is_fast ... ok
test human_verified_content_outranks_unverified_content_for_the_same_query ... ok
test annotation_boost_outranks_plain_content_end_to_end ... ok
test agent_authored_content_is_penalized_end_to_end ... ok
test warm_queries_are_fast ... ok
test retrievability_regression_set_is_fully_recalled ... ok
test rebuilding_from_scratch_is_deterministic ... ok
test two_writers_converge_without_coordination ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

**Retrievability regression set** (`searchidx/tests/fixtures/retrievability.json`):
**50 cases, 50/50 recalled (100%)** — counted with
`python3 -c "import json; print(len(json.load(open('searchidx/tests/fixtures/retrievability.json'))))"`.
As the fixture file's own doc comment notes, this is a deliberately small
synthetic-corpus regression set (a tokenizer/ranking blind-spot guardrail),
not the spec's 100-case target sized against the real 8,826-file vault — see
task-11's report for that honest accounting.

## 7 · Summary table (spec §7 "实测" column)

| Metric | Measured | Note |
| --- | --- | --- |
| Binary delta (baseline → HEAD) | **+4.10 MB / +3.91 MiB** | bare `notemd` executable, release+LTO+strip, §1 |
| Under 5.0 MB budget? | **Yes** | |
| Download size (est.) | ~11 MB | inferred, see §1 caveat — not directly measured |
| Installed size (est.) | ~15 MB | inferred, see §1 caveat — not directly measured |
| Index size / corpus size | 102,400 B / 144 KB ≈ 0.71× | 36-file synthetic corpus, not the 10k-file anchor, §2 |
| Cold build (CLI, 36 files) | ≈150–170 ms | dominated by one-time jieba dict decompression, §3 |
| Unchanged sweep (CLI) | <10 ms (below timer resolution) | stat fast-path no-op, §4 |
| Query p50 / p95 | 17 µs / 18 µs | in-process, warm, 36-file corpus, §5 |
| Retrievability regression set | 50/50 (100%) | §6 |
| Rebuild-from-scratch determinism | pass | §6 |
| Concurrent-writer convergence | pass | §6 |
