//! Provenance tiering for retrieval ranking (spec: `docs/superpowers/specs/
//! 2026-08-11-md-origin-tiering-design.md`, §3).
//!
//! `origin` classifies every indexed `.md` file into one of three tiers so
//! later ranking can weigh "what you wrote" above "what an agent produced"
//! above "raw material an agent has to read" (CLAUDE.md belief 1). It is
//! **derived at index time and never written back to the file** (belief 2,
//! file-over-app) — the vault's frontmatter stays exactly what its author
//! wrote; `origin` only ever lives in the index row.
//!
//! §3's rule table is ordered and **first match wins**. The order below must
//! match the spec table exactly; each rule below carries the rationale for
//! why it sits where it does, because reordering two rules silently changes
//! real files' tiers (see the two priority tests at the bottom of this file).

use crate::frontmatter::Frontmatter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// You wrote it, or you signed it. Ranked highest.
    Human,
    /// An agent produced it — summaries, answers, argument docs. Reproducible,
    /// so it is not irreplaceable the way `Human` or `Source` are.
    Derived,
    /// Raw material an agent (or import pipeline) has to read but did not
    /// judge — ebook exports, transcripts, blog captures. Not your judgment,
    /// but also not disposable: losing it means re-fetching it.
    Source,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Human => "human",
            Origin::Derived => "derived",
            Origin::Source => "source",
        }
    }

    pub fn from_str(s: &str) -> Option<Origin> {
        match s {
            "human" => Some(Origin::Human),
            "derived" => Some(Origin::Derived),
            "source" => Some(Origin::Source),
            _ => None,
        }
    }
}

/// §3.1's type → tier mapping. Kept in sync with `CONCEPT_TYPE` in
/// `src/lib/okf/concept.ts` by the cross-language test at the bottom of this
/// file, which reads `tests/fixtures/origin/concept-types.json` — a fixture
/// regenerated from `concept.ts` by `pnpm gen:origin-types` (see that script
/// and the sibling TS-side staleness test in
/// `src/lib/okf/concept-origin-sync.test.ts`).
///
/// That sync test only catches a type that was **added to the registry and
/// never given a tier here** — it cannot catch a type mapped to the *wrong*
/// tier. Whoever registers a new `CONCEPT_TYPE` value must add it below
/// themselves and pick the right tier; see the registration comment in
/// `concept.ts` for the same caveat from the other side.
fn mapped_type_origin(concept_type: &str) -> Option<Origin> {
    match concept_type {
        "Note" | "Outline Note" | "Daily Note" | "Wiki Page" | "Idea" | "Vault Conventions" => {
            Some(Origin::Human)
        }
        "Book Summary" | "Answer" | "Idea Proof" | "Reading Report" | "Decision Board" | "Decision Archive" => {
            Some(Origin::Derived)
        }
        "Book" => Some(Origin::Source),
        _ => None,
    }
}

/// Derive a file's provenance tier. `rel_path` is vault-relative and
/// `/`-separated (see `norm::rel_path`). `sync_dir` is the sync mirror
/// directory prefix (default `"sync"`, see the vault settings for project A) —
/// files under it are mirrored copies of something that lives outside the
/// vault, not something written in it.
///
/// **`Some(&Frontmatter::default())` is not equivalent to `None`.** Rule 6
/// only fires on `None` — a file that genuinely has no `---` frontmatter
/// block at all. `frontmatter::parse` never fails; a present-but-empty or
/// present-but-irrelevant frontmatter block still parses to a
/// `Frontmatter::default()`-shaped value, and passing that here as
/// `Some(...)` skips rule 6 and falls through to rule 7's `Derived`. If your
/// call site has already collapsed "no frontmatter" and "empty frontmatter"
/// into one value (e.g. `fm_raw.map(parse).unwrap_or_default()`, as
/// `chunk::parse_file` does today), you must pass `None` yourself when
/// `fm_raw` was `None` to get rule 6's behavior — see
/// `some_default_frontmatter_is_not_the_same_as_none` below, which pins
/// today's `Derived` result for that case as a trap for exactly this bug.
///
/// Related gray area, not a rule: `---\n---` (a frontmatter block that is
/// present but empty) also parses to `Frontmatter::default()` and — same as
/// above — resolves to `Derived` via rule 7, even though rule 7's rationale
/// ("有 frontmatter、有类型" — has frontmatter, has a type) doesn't actually
/// describe a file with no type. Spec §3 does not define a separate rule for
/// "frontmatter present but empty"; this is a known unmodeled case, not an
/// intentional decision — do not build on it without checking the spec.
pub fn derive(rel_path: &str, fm: Option<&Frontmatter>, sync_dir: &str) -> Origin {
    // Rule 1 — `.note.md` is your annotation container by construction (the
    // outline/sidecar-notes convention): even before anything is written into
    // it, it exists because *you* opened it to hold your marginalia. This is
    // a file-level judgment and is deliberately blind to what is written
    // inside — see `note_md_beats_generated_by` below for why it must outrank
    // rule 2's block-level `generated.by` signal, not the other way around.
    if rel_path.ends_with(".note.md") {
        return Origin::Human;
    }

    if let Some(fm) = fm {
        // Rule 2 — `generated.by` is a first-hand claim about who produced
        // *this whole document* (OKF §5.2/§7). A `human:` actor means a
        // person authored or transcribed it by hand even though they bothered
        // to stamp it; anything else (`<producer>/<version>`, `process:<id>`)
        // means a generator wrote it.
        if let Some(by) = fm.generated_by.as_deref() {
            return if by.starts_with("human:") { Origin::Human } else { Origin::Derived };
        }

        // Rule 3 — `verified.by` with a `human:` prefix is a person putting
        // their name on the document after the fact (OKF §5.2/§7's
        // human-confirmation case). That is as strong a human signal as
        // authorship, so it earns the same tier as rule 2's human case.
        if fm.human_verified {
            return Origin::Human;
        }

        // Rule 4 — a registered `type` is the vault's own classification of
        // what kind of document this is, and it is more specific than "is it
        // in the mirror directory" (rule 5): a mirrored ebook summary is
        // still a summary. See `a_registered_type_beats_the_mirror_dir` below
        // for why this must run before rule 5, not after.
        if let Some(t) = fm.concept_type.as_deref() {
            if let Some(o) = mapped_type_origin(t) {
                return o;
            }
        }
    }

    // Rule 5 — the sync mirror directory holds copies of files that live
    // outside the vault (CLAUDE.md belief 4); nothing under it was written in
    // the vault, so absent a stronger signal above it defaults to raw
    // material. This check does not require frontmatter, unlike rules 2-4, so
    // it also catches mirrored files with no frontmatter at all.
    let dir = sync_dir.trim_matches('/');
    if !dir.is_empty() && (rel_path == dir || rel_path.starts_with(&format!("{dir}/"))) {
        return Origin::Source;
    }

    // Rule 6 — a bare `.md` with no frontmatter at all carries no claim of
    // authorship, generation, verification, or type — nothing above matched.
    // The tier here is a **deliberate misclassification**, not a neutral
    // default: it judges "source" (raw material), not "human" (your
    // judgment). A hand-written note with no frontmatter is misfiled by this
    // rule and loses its ranking boost — but the alternative direction is
    // worse. If the default were `human`, every faceless AI dump that forgot
    // (or was never given) a `generated.by` stamp would land in the tier
    // meant to be most trusted, silently diluting the one signal this whole
    // feature exists to protect. The fix for the misfiled note is cheap and
    // already a project convention: add frontmatter. The fix for a
    // human-tier flooded with AI output is not cheap — it is this feature
    // failing at its one job. See spec §3.2 and §9.
    if fm.is_none() {
        return Origin::Source;
    }

    // Rule 7 — frontmatter exists (so something deliberately produced this
    // document — OKF §11 forbids treating an unrecognized `type` as a reason
    // to reject a document) but its `type` is not one this vault has
    // registered. That is exactly the shape of a plugin's fresh output before
    // anyone has taught this crate about the new type: assume `derived`
    // rather than `source`, because "has frontmatter" is itself evidence of a
    // deliberate producer, not raw capture.
    Origin::Derived
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(s: &str) -> Frontmatter {
        crate::frontmatter::parse(s)
    }

    #[test]
    fn rule1_note_md_is_always_human() {
        assert_eq!(derive("a.note.md", None, "sync"), Origin::Human);
    }
    #[test]
    fn rule2_generated_by_agent_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: claude/1")), "sync"), Origin::Derived);
    }
    #[test]
    fn rule2_generated_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: human:bruce")), "sync"), Origin::Human);
    }
    #[test]
    fn rule3_verified_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("verified:\n  by: human:me")), "sync"), Origin::Human);
    }
    #[test]
    fn rule4_maps_registered_types() {
        assert_eq!(derive("a.md", Some(&fm("type: Note")), "sync"), Origin::Human);
        assert_eq!(derive("a.md", Some(&fm("type: Book Summary")), "sync"), Origin::Derived);
        assert_eq!(derive("a.md", Some(&fm("type: Book")), "sync"), Origin::Source);
    }
    #[test]
    fn rule5_mirror_dir_is_source() {
        assert_eq!(derive("sync/x/a.md", Some(&fm("title: t")), "sync"), Origin::Source);
    }
    /// Rule 5 matches the mirror dir as a `/`-bounded path prefix, not a bare
    /// string prefix — `synced/` must not match `sync_dir = "sync"`, and a
    /// `sync` segment nested deeper in the path or an empty `sync_dir` must
    /// not match at all. A refactor to `rel_path.starts_with(dir)` (dropping
    /// the `/`) would pass `rule5_mirror_dir_is_source` above while silently
    /// misclassifying `synced/notes.md` as `Source` — this pins the negative
    /// space so that refactor goes red instead.
    #[test]
    fn rule5_mirror_dir_does_not_match_a_lookalike_prefix() {
        let title = fm("title: t");
        assert_eq!(derive("synced/a.md", Some(&title), "sync"), Origin::Derived);
        assert_eq!(derive("my-sync/a.md", Some(&title), "sync"), Origin::Derived);
        assert_eq!(derive("a/sync/b.md", Some(&title), "sync"), Origin::Derived);
        assert_eq!(derive("sync/x/a.md", Some(&title), ""), Origin::Derived);
    }
    #[test]
    fn rule6_no_frontmatter_is_source() {
        assert_eq!(derive("a.md", None, "sync"), Origin::Source);
    }
    /// Pins the trap documented on `derive`'s doc comment: `Some(&Frontmatter
    /// ::default())` is NOT the same input as `None`, even though both
    /// represent "nothing interesting in the frontmatter" to a casual reader.
    /// Rule 6 is keyed off `fm.is_none()`, so this falls through to rule 7 and
    /// resolves to `Derived` — the opposite of `rule6_no_frontmatter_is_source`
    /// above despite looking equivalent. `chunk::parse_file` already produces
    /// exactly this shape today (`fm_raw.map(parse).unwrap_or_default()`
    /// collapses "no frontmatter" and "empty frontmatter" into one
    /// `Frontmatter` value before it would reach `derive`), so a caller that
    /// forwards that value as `Some(&fm)` unconditionally — instead of
    /// checking `fm_raw.is_some()` first — silently inverts spec §3.2's
    /// deliberate misclassification direction for the bulk of frontmatter-less
    /// files. This test does not assert that `Derived` is *correct*; it pins
    /// what the code does today so a future change to this behavior is a
    /// deliberate, reviewed decision rather than an accidental regression.
    #[test]
    fn some_default_frontmatter_is_not_the_same_as_none() {
        assert_eq!(derive("a.md", Some(&Frontmatter::default()), "sync"), Origin::Derived);
    }
    #[test]
    fn rule7_unknown_type_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("type: Some Plugin Thing")), "sync"), Origin::Derived);
    }

    /// Priority: rule 1 beats rule 2 — an agent wrote a reply into *your*
    /// annotation container, but the container is still yours. File-level
    /// `human` and block-level `agent_by` are two different layers; only the
    /// latter tracks who wrote which specific node (see `block::Block::agent_by`).
    #[test]
    fn note_md_beats_generated_by() {
        assert_eq!(derive("a.note.md", Some(&fm("generated:\n  by: claude/1")), "sync"), Origin::Human);
    }
    /// Priority: rule 4 beats rule 5 — a summary sitting in the mirror
    /// directory is still an AI summary, not raw material just because of
    /// where it lives.
    #[test]
    fn a_registered_type_beats_the_mirror_dir() {
        assert_eq!(derive("sync/s.md", Some(&fm("type: Book Summary")), "sync"), Origin::Derived);
    }

    #[test]
    fn as_str_and_from_str_round_trip() {
        for o in [Origin::Human, Origin::Derived, Origin::Source] {
            assert_eq!(Origin::from_str(o.as_str()), Some(o));
        }
        assert_eq!(Origin::from_str("nonsense"), None);
    }

    /// Cross-language sync with `CONCEPT_TYPE` (src/lib/okf/concept.ts) —
    /// spec §3.1. `tests/fixtures/origin/concept-types.json` is generated
    /// from `concept.ts` by `pnpm gen:origin-types`; if a type is added to
    /// the registry and this file is regenerated without a matching arm added
    /// to `mapped_type_origin`, this test goes red. It cannot tell you the
    /// arm is in the *right* tier — see the comment on `mapped_type_origin`.
    #[test]
    fn every_registered_concept_type_has_a_mapped_origin() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/origin/concept-types.json");
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let types: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert!(!types.is_empty(), "fixture must not be empty — {}", path.display());
        for t in &types {
            assert!(
                mapped_type_origin(t).is_some(),
                "CONCEPT_TYPE `{t}` has no origin tier in searchidx::origin::mapped_type_origin — \
                 add it to the match in origin.rs (spec §3.1)"
            );
        }
    }
}
