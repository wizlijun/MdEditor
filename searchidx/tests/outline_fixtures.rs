//! The Rust chunker and the TypeScript `parseOutline` must agree on which line
//! belongs to which node. Two implementations of one format drift silently, and
//! a drift here means the same `.note.md` is a different tree depending on who
//! read it — unacceptable under "one vault, many agents". So both sides are
//! pinned to the same fixture files and the same expected JSON.
//! The TS half lives in src/lib/outline/cross-lang-fixtures.test.ts.

use std::path::Path;

#[test]
fn rust_chunker_matches_the_shared_fixture_expectations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outline");
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();

    for (name, want) in expected.as_object().unwrap() {
        let raw = std::fs::read_to_string(dir.join(name)).unwrap();
        let text = searchidx::norm::strip_cr(&raw);
        let got: Vec<serde_json::Value> = searchidx::outline::chunk(&text, 1)
            .into_iter()
            .filter(|b| b.level == searchidx::block::BlockLevel::Line)
            .map(|b| {
                serde_json::json!({
                    "line_start": b.line_start, "line_end": b.line_end,
                    "breadcrumb": b.breadcrumb, "text": b.text,
                    "is_annotation": b.is_annotation, "agent_by": b.agent_by,
                })
            })
            .collect();
        assert_eq!(&serde_json::Value::Array(got), want, "fixture {name} diverged");
    }
}
