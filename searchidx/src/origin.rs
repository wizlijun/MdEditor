//! Provenance tiering for retrieval ranking (spec: `docs/superpowers/specs/
//! 2026-08-11-md-origin-tiering-design.md`, §3; rules 5 and 6 superseded by
//! `docs/superpowers/specs/2026-08-12-source-globs-and-transcript-indexing-
//! design.md`, §3 — that document's rule table, 5′/6′, is what the code
//! below implements).
//!
//! `origin` classifies every indexed file into one of four tiers so
//! later ranking can weigh "what you wrote" above "what an agent produced"
//! above "raw material an agent has to read" above "nobody has said who
//! wrote this" (CLAUDE.md belief 1). It is **derived at index time and never
//! written back to the file** (belief 2, file-over-app) — the vault's
//! frontmatter stays exactly what its author wrote; `origin` only ever lives
//! in the index row.
//!
//! "Every indexed file", not just `.md`: since C-T4/C-T5, `chunk::parse_file`
//! also calls `derive` for `.srt`/`.vtt`/`.txt`. Those arrive with `None`
//! frontmatter by construction (they have none to parse — see the format
//! dispatch in `chunk::parse_file`), and they are only ever indexed *inside*
//! a source glob, so rule 5′ necessarily fires before rule 6′ can: a
//! transcript is always `Source`, never `Unlabeled`. Spec §3 states that as
//! an invariant; it is a consequence of the scan gate, not a rule here.
//!
//! §3's rule table is ordered and **first match wins**. The order below must
//! match the spec table exactly; each rule below carries the rationale for
//! why it sits where it does, because reordering two rules silently changes
//! real files' tiers (see the priority tests at the bottom of this file).

use crate::frontmatter::Frontmatter;
use crate::globs::SourceGlobs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// You wrote it, or you signed it. Ranked highest.
    Human,
    /// An agent produced it — summaries, answers, argument docs. Reproducible,
    /// so it is not irreplaceable the way `Human` or `Source` are.
    Derived,
    /// Raw material an agent (or import pipeline) has to read but did not
    /// judge — ebook exports, transcripts, blog captures. Not your judgment,
    /// but also not disposable: losing it means re-fetching it. Membership
    /// is decided by the user's source-glob patterns (rule 5′, spec §4.1),
    /// not by absence of frontmatter — that used to be the proxy and it
    /// swept in every unlabeled file whether or not it was actually raw
    /// material (see `Unlabeled` below).
    Source,
    /// Nobody has claimed this file: no frontmatter, and it did not match a
    /// source-glob pattern either. This is **not a judgment about the
    /// content** — an unlabeled file might well be your own writing — it is
    /// an honest statement that no signal above fired. Ranked lowest of all
    /// four tiers on purpose (spec §3.1, ×0.3) to create pressure to label
    /// it one way or the other: add frontmatter, or add it to a source-glob
    /// pattern if it really is raw material.
    Unlabeled,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Human => "human",
            Origin::Derived => "derived",
            Origin::Source => "source",
            Origin::Unlabeled => "unlabeled",
        }
    }

    pub fn from_str(s: &str) -> Option<Origin> {
        match s {
            "human" => Some(Origin::Human),
            "derived" => Some(Origin::Derived),
            "source" => Some(Origin::Source),
            "unlabeled" => Some(Origin::Unlabeled),
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
        "Note" | "Outline Note" | "Daily Note" | "Wiki Page" | "Idea" | "Next" | "Task"
        | "Vault Conventions" | "User Profile" | "Memory" | "Trace Request" => Some(Origin::Human),
        "Book Summary" | "Book Topic Index" | "Answer" | "Idea Proof" | "Reading Report"
        | "Decision Board" | "Decision Archive" | "Trace Report" => Some(Origin::Derived),
        "Book" | "Trace Material" => Some(Origin::Source),
        _ => None,
    }
}

/// Derive a file's provenance tier. `rel_path` is vault-relative and
/// `/`-separated (see `norm::rel_path`). `globs` is the user's configured
/// source-glob patterns (`globs::SourceGlobs`, spec §4.1) — rule 5′ asks it
/// whether `rel_path` counts as raw source material. An empty `SourceGlobs`
/// (`SourceGlobs::default()`, the state before the user has configured
/// anything) matches nothing, per that type's own contract — it is not
/// "matches everything" — so rule 5′ simply never fires and every path falls
/// through to rule 6′/7 until the user names patterns.
///
/// **`Some(&Frontmatter::default())` is not equivalent to `None`.** Rule 6′
/// only fires on `None` — a file that genuinely has no `---` frontmatter
/// block at all. `frontmatter::parse` never fails; a present-but-empty or
/// present-but-irrelevant frontmatter block still parses to a
/// `Frontmatter::default()`-shaped value, and passing that here as
/// `Some(...)` skips rule 6′ and falls through to rule 7's `Derived`. If your
/// call site has already collapsed "no frontmatter" and "empty frontmatter"
/// into one value (e.g. `fm_raw.map(parse).unwrap_or_default()`, as
/// `chunk::parse_file` does today), you must pass `None` yourself when
/// `fm_raw` was `None` to get rule 6′'s behavior — see
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
pub fn derive(rel_path: &str, fm: Option<&Frontmatter>, globs: &SourceGlobs) -> Origin {
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
        //
        // Known blind spot, recorded rather than fixed (the order is
        // spec-normative, §3): this rationale was written about AI *summaries*
        // — a generator that read something and produced judgment about it —
        // and does not account for a mechanical conversion pipeline over raw
        // material. Because rule 2 precedes rule 4, a generator-stamped
        // `type: Book` classifies `Derived`, even though spec §1 names ebook
        // exports as the archetype of raw source material and `Book` is the
        // only type mapped to `Source`. It is latent today only because
        // `plugins-src/ebook-import/backend/src/bookconf.rs` writes `type:
        // Book` + `sources:` and no `generated:` — and
        // `docs/okf-v0.2-conformance-audit.md` is pushing producers toward
        // stamping `generated`, so one added line there would silently move
        // every imported book out of the source tier. Revisit the ordering
        // with the spec, not around it.
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
        // what kind of document this is, and it is more specific than "does
        // it match a source-glob pattern" (rule 5′): a summary that happens
        // to sit inside a designated source directory is still a summary.
        // See `a_registered_type_beats_a_source_glob` below for why this must
        // run before rule 5′, not after.
        if let Some(t) = fm.concept_type.as_deref() {
            if let Some(o) = mapped_type_origin(t) {
                return o;
            }
        }
    }

    // Rule 5′ — hits one of the user's configured source-glob patterns
    // (spec §4.1, `globs::SourceGlobs`). This replaces the old sync-mirror-
    // directory special case (former rule 5): that directory check was one
    // proxy for "raw material I didn't write," when the actual criterion
    // this product wants is "the user has designated this as raw material" —
    // globs express that directly and more broadly (ebook exports,
    // transcript directories, anything else the user names), without a
    // second parallel mechanism (a setting + a `ScanOptions` field + its own
    // `meta` stamp) duplicating what a pattern can already say. This check
    // does not require frontmatter, unlike rules 2-4, so it also catches a
    // matched file with no frontmatter at all — see
    // `a_matched_path_without_frontmatter_is_source_not_unlabeled` below for
    // why that must win over rule 6′, not the other way around.
    if globs.matches(rel_path) {
        return Origin::Source;
    }

    // Rule 6′ — a bare `.md` with no frontmatter at all, that also didn't
    // match a source-glob pattern (rule 5′ already returned above if it
    // had), carries no claim of authorship, generation, verification, type,
    // or designated raw material — nothing above matched. The tier here is
    // still a **deliberate non-default**, not a coin flip: it must not
    // resolve to `Human`. A hand-written note with no frontmatter is
    // misfiled by this rule and loses its ranking boost — but the
    // alternative direction is worse. If the default were `human`, every
    // faceless AI dump that forgot (or was never given) a `generated.by`
    // stamp would land in the tier meant to be most trusted, silently
    // diluting the one signal this whole feature exists to protect.
    //
    // Where this differs from the pre-2026-08-12 rule 6: that version also
    // avoided `Human`, but resolved the file to `Source` instead, reasoning
    // that "raw material" was at least a safer wrong guess than "your
    // judgment." That was still a guess dressed up as a classification — a
    // frontmatter-less file is no more evidence it is raw source material
    // than it is evidence you wrote it. `Unlabeled` says the honest thing
    // instead: nobody has claimed this file, in either direction. It is
    // ranked lowest of all four tiers (spec §3.1, ×0.3) — lower than
    // `Source` — specifically to create pressure to resolve that: add
    // frontmatter, or add the file to a source-glob pattern if it genuinely
    // is raw material. See spec §3.2 and §9 (2026-08-12 design).
    if fm.is_none() {
        return Origin::Unlabeled;
    }

    // Rule 7 — frontmatter exists (so something deliberately produced this
    // document — OKF §11 forbids treating an unrecognized `type` as a reason
    // to reject a document) but its `type` is not one this vault has
    // registered. That is exactly the shape of a plugin's fresh output before
    // anyone has taught this crate about the new type: assume `derived`
    // rather than `source` or `unlabeled`, because "has frontmatter" is
    // itself evidence of a deliberate producer, not raw capture and not
    // silence.
    Origin::Derived
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(s: &str) -> Frontmatter {
        crate::frontmatter::parse(s)
    }

    fn globs(p: &[&str]) -> SourceGlobs {
        crate::globs::parse(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn rule1_note_md_is_always_human() {
        assert_eq!(derive("a.note.md", None, &globs(&[])), Origin::Human);
    }
    #[test]
    fn rule2_generated_by_agent_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: claude/1")), &globs(&[])), Origin::Derived);
    }
    #[test]
    fn rule2_generated_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: human:bruce")), &globs(&[])), Origin::Human);
    }
    #[test]
    fn rule3_verified_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("verified:\n  by: human:me")), &globs(&[])), Origin::Human);
    }
    #[test]
    fn rule4_maps_registered_types() {
        assert_eq!(derive("a.md", Some(&fm("type: Note")), &globs(&[])), Origin::Human);
        assert_eq!(
            derive("USER.md", Some(&fm("type: User Profile")), &globs(&[])),
            Origin::Human
        );
        assert_eq!(
            derive("MEMORY.md", Some(&fm("type: Memory")), &globs(&[])),
            Origin::Human
        );
        assert_eq!(derive("a.md", Some(&fm("type: Book Summary")), &globs(&[])), Origin::Derived);
        assert_eq!(derive("a.md", Some(&fm("type: Book")), &globs(&[])), Origin::Source);
    }
    #[test]
    fn next_ledger_is_human_judgment() {
        assert_eq!(
            derive(
                "thinking/next.md",
                Some(&fm("type: Next")),
                &globs(&[])
            ),
            Origin::Human
        );
    }
    #[test]
    fn unsigned_task_is_human_but_agent_generated_task_is_derived() {
        assert_eq!(
            derive(
                "inbox/tasks/manual-task.md",
                Some(&fm("type: Task")),
                &globs(&[])
            ),
            Origin::Human
        );
        assert_eq!(
            derive(
                "inbox/tasks/generated-task.md",
                Some(&fm("type: Task\ngenerated:\n  by: daily-summary-agent/1")),
                &globs(&[])
            ),
            Origin::Derived
        );
    }
    #[test]
    fn trace_types_map_report_derived_material_source() {
        assert_eq!(derive("traces/a.md", Some(&fm("type: Trace Report")), &globs(&[])), Origin::Derived);
        assert_eq!(derive("traces/a/01-b.md", Some(&fm("type: Trace Material")), &globs(&[])), Origin::Source);
    }
    #[test]
    fn rule5_a_matched_path_is_source() {
        assert_eq!(derive("ebook/a.md", Some(&fm("title: t")), &globs(&["ebook/**"])), Origin::Source);
    }
    /// 这是本次的核心修正:缺 frontmatter 不再等同于原始资料。
    #[test]
    fn rule6_no_frontmatter_and_no_match_is_unlabeled() {
        assert_eq!(derive("notes/a.md", None, &globs(&["ebook/**"])), Origin::Unlabeled);
    }
    /// 规则 5′ 压过规则 6′ —— 指定目录里没有 frontmatter 的文件是原始资料,
    /// 不是未标注。
    #[test]
    fn a_matched_path_without_frontmatter_is_source_not_unlabeled() {
        assert_eq!(derive("ebook/a.md", None, &globs(&["ebook/**"])), Origin::Source);
    }
    /// Pins the trap documented on `derive`'s doc comment: `Some(&Frontmatter
    /// ::default())` is NOT the same input as `None`, even though both
    /// represent "nothing interesting in the frontmatter" to a casual reader.
    /// Rule 6′ is keyed off `fm.is_none()`, so this falls through to rule 7 and
    /// resolves to `Derived` — the opposite of `rule6_no_frontmatter_and_no_match_is_unlabeled`
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
        assert_eq!(derive("a.md", Some(&Frontmatter::default()), &globs(&[])), Origin::Derived);
    }
    #[test]
    fn rule7_unknown_type_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("type: Some Plugin Thing")), &globs(&[])), Origin::Derived);
    }

    /// Priority: rule 1 beats rule 2 — an agent wrote a reply into *your*
    /// annotation container, but the container is still yours. File-level
    /// `human` and block-level `agent_by` are two different layers; only the
    /// latter tracks who wrote which specific node (see `block::Block::agent_by`).
    #[test]
    fn note_md_beats_generated_by() {
        assert_eq!(derive("a.note.md", Some(&fm("generated:\n  by: claude/1")), &globs(&[])), Origin::Human);
    }
    /// The classification this whole feature exists to get right, end to end
    /// through the frontmatter reader, in the exact shape
    /// `src-tauri/templates/AGENTS.md` tells every agent to write: a `type:`
    /// from its own type table plus a **flow-form** `generated:` stamp. Until
    /// the reader learned the flow form, rule 2 never fired for these files,
    /// rule 4 caught them on `Note`, and agent output landed in `Human` —
    /// spec §3.2's explicitly-named expensive direction ("AI 产物混进最该被
    /// 信任的一层"). Written against `frontmatter::parse` rather than a
    /// hand-built `Frontmatter` on purpose: a hand-built value cannot regress
    /// this, because the defect was entirely in the reader.
    #[test]
    fn an_agent_stamp_in_agents_md_flow_form_is_derived_not_human() {
        let f = fm("type: Note\ngenerated: { by: claude-code/opus-5, at: 2026-08-03T14:22:00Z }");
        assert_eq!(
            derive("notes/a.md", Some(&f), &globs(&[])),
            Origin::Derived,
            "a flow-form `generated.by` must fire rule 2 before rule 4 maps `Note` to Human"
        );
        // The mirror image: a human's own inline signature must not lose its
        // tier either (rule 3), which costs x1.25 AND the x1.1 human_verified
        // boost when it is missed.
        let v = fm("type: Book Summary\nverified: [{ by: human:bruce, at: 2026-08-03T14:22:00Z }]");
        assert_eq!(derive("notes/b.md", Some(&v), &globs(&[])), Origin::Human);
        assert!(v.human_verified, "and the x1.1 boost must survive too");
    }

    /// Priority: rules 2/4 beat rule 5′ — a summary sitting inside a
    /// designated source-glob directory is still an AI summary, not raw
    /// material just because of where it lives. (Takes over from the old
    /// `a_registered_type_beats_the_mirror_dir`, which pinned the same
    /// priority against the now-removed sync-mirror-directory mechanism.)
    #[test]
    fn a_registered_type_beats_a_source_glob() {
        assert_eq!(
            derive("ebook/s.md", Some(&fm("type: Book Summary")), &globs(&["ebook/**"])),
            Origin::Derived
        );
    }
    #[test]
    fn a_generated_stamp_beats_a_source_glob() {
        assert_eq!(
            derive("ebook/s.md", Some(&fm("generated:\n  by: claude/1")), &globs(&["ebook/**"])),
            Origin::Derived
        );
    }

    #[test]
    fn as_str_and_from_str_round_trip() {
        for o in [Origin::Human, Origin::Derived, Origin::Source, Origin::Unlabeled] {
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

    /// 三态可分 —— 这是「人写署名」整件事存在的理由。此前只有前两态,
    /// 而且第一态没人写得出来。
    #[test]
    fn the_three_states_of_authorship_are_distinguishable() {
        let human = fm("type: Note\ngenerated:\n  by: human:bruce\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(derive("a.md", Some(&human), &globs(&[])), Origin::Human);

        let machine = fm("type: Note\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(
            derive("b.md", Some(&machine), &globs(&[])),
            Origin::Derived,
            "a generator stamp must not inherit Note's Human tier"
        );

        let unclaimed = fm("type: Note");
        assert_eq!(
            derive("c.md", Some(&unclaimed), &globs(&[])),
            Origin::Human,
            "no stamp falls back to the type mapping (rule 4) — unchanged behaviour"
        );
    }

    /// 不签清单的守卫(spec §2.2)。`book.md` 至今不带 `generated`,所以它走
    /// 规则 4 落在 `Source`。谁哪天给 ebook 导入补了一行 `generated`,
    /// 规则 2 会抢在规则 4 前面把每一本导入的书悄悄挪进 `Derived` —— 这条
    /// 测试就是那一刻的红灯。真要改,去改规则顺序和 spec,不要绕过它。
    #[test]
    fn an_imported_book_stays_source_because_nobody_stamps_it() {
        assert_eq!(derive("books/x.md", Some(&fm("type: Book")), &globs(&[])), Origin::Source);

        let stamped = fm("type: Book\ngenerated:\n  by: process:ebook-import\n  at: 2026-08-20T10:31:00.000Z");
        assert_eq!(
            derive("books/x.md", Some(&stamped), &globs(&[])),
            Origin::Derived,
            "rule 2 precedes rule 4 — this is why ebook-import must not stamp `generated`"
        );
    }

    /// 导入页(roam-import)不签 `generated`,照样落在 Human 档 —— 搬运不是撰写,
    /// 但内容确实是人在别处写的。两条路都通:`.note.md` 的后缀由规则 1 直接兜住,
    /// 后缀之外则由规则 4 的 type 映射兜住。断言分开写,免得一条掩盖另一条。
    #[test]
    fn imported_pages_reach_human_without_any_stamp() {
        let page = fm("type: Wiki Page\ntitle: 回顾系统\ncreated: 2026-08-02T00:00:00.000Z");
        // 后缀命中规则 1(与 frontmatter 无关),这是 roam 导入页的实际形态。
        assert_eq!(derive("wikipage/回顾系统.note.md", Some(&page), &globs(&[])), Origin::Human);
        // 去掉后缀后规则 1 不再触发,靠规则 4 的 `Wiki Page` → Human 映射。
        assert_eq!(derive("wikipage/回顾系统.md", Some(&page), &globs(&[])), Origin::Human);
    }
}
