// Search panel grouping (task B-T7, design spec §5). Pure function, no
// Svelte import — the panel calls this and renders `HitGroup[]`, but the
// grouping logic itself is unit-testable without a component harness.
//
// The user's stated intent, verbatim: 「我期望突出纯人写的和纯原始资料。中间的
// 那些可以通过类型来区分。」 Two poles pinned at the ends of the list, the
// middle subdivided by `concept_type` — not a fixed three-group layout: the
// group count follows whatever types are actually present in the results.
//
// **UI-only.** `notemd search`'s default output stays flat `path:line:text`
// (see `src-tauri/tests/search_cli_contract.rs`) — this module has no
// caller on that path.
import type { SearchHit } from './api'

export type HitGroupKind = 'human' | 'derivedType' | 'derivedOther' | 'source'

export interface HitGroup {
  kind: HitGroupKind
  /** Only set for `kind === 'derivedType'` — the raw `concept_type` string
   *  (e.g. `'Book Summary'`), used verbatim as the group's label. Concept
   *  types are an open, plugin-extensible vocabulary (`CONCEPT_TYPE` in
   *  `src/lib/okf/concept.ts`), not a fixed enum, so this is never
   *  translated — a plugin can introduce a new type without touching i18n. */
  conceptType?: string
  hits: SearchHit[]
}

/**
 * Groups already-ranked hits for display. Order is fixed:
 *
 *   human            (origin = 'human', the pole for "what you wrote")
 *     <type A>        (origin = 'derived', subdivided by conceptType,
 *     <type B>         ordered by each type's first appearance in `hits` —
 *     …                 i.e. the type of the highest-scoring hit surfaces
 *                        first, since `hits` arrives score-sorted)
 *     其他            (derived hits with no conceptType — rule 7's
 *                       unregistered/typeless case — always last among the
 *                       derived groups, never interleaved with named types)
 *   source            (origin = 'source' OR 'unlabeled' — see the TODO(C-T10)
 *                       below on why 'unlabeled' is folded in here for now)
 *
 * A group that would be empty is omitted entirely, not rendered empty — so
 * the group count is exactly `(human present ? 1 : 0) + (distinct derived
 * types present) + (untyped derived present ? 1 : 0) + (source-or-unlabeled
 * present ? 1 : 0)`, which is "more than three" as soon as two or more
 * derived types show up in one result set (by design — see the module doc
 * comment).
 *
 * TODO(C-T10): `origin = 'unlabeled'` (2026-08-12 design, C-T2 — spec §3
 * rule 6′) is deliberately folded into the `source` group rather than given
 * its own `HitGroupKind`. This is an interim measure, not the intended final
 * shape: `Unlabeled` and `Source` are different claims (§9's "known signal"
 * table), and this file's own module doc says the goal is exactly two poles
 * plus a derived middle, not three. The reason to fold rather than leave it
 * unhandled: before this change, an `origin: 'unlabeled'` hit matched none of
 * the branches below and fell through to `derivedOther` — rendered under the
 * *AI-produced* heading, which is a strictly stronger false claim than
 * "raw material" (an unlabeled file might be your own unsigned writing; it
 * is definitely not evidence an agent produced it). Folding into `source`
 * is not correct either, but it reproduces this codebase's pre-C-T2 behavior
 * for these exact files (frontmatter-less `.md` used to classify `Source`
 * under the retired rule 6) rather than inventing a new, worse mislabel.
 * C-T10 owns building the real fourth group (a `HitGroupKind` variant, an
 * i18n label in `SearchPanel.svelte`'s `groupLabel` switch, and presumably
 * its own position in the ordering — spec §3.1 ranks it below `source`, so
 * it likely belongs after `source` in the list above, not merged into it).
 *
 * Within each group, hits keep the relative order they arrived in — this
 * function never re-sorts by score; that already happened upstream in
 * `searchidx::query::finish`.
 */
export function groupHits(hits: SearchHit[]): HitGroup[] {
  const human: SearchHit[] = []
  const source: SearchHit[] = []
  const derivedOther: SearchHit[] = []
  const derivedTypeOrder: string[] = []
  const derivedByType = new Map<string, SearchHit[]>()

  for (const hit of hits) {
    if (hit.origin === 'human') {
      human.push(hit)
    } else if (hit.origin === 'source' || hit.origin === 'unlabeled') {
      // TODO(C-T10): see the module doc comment above — this is a temporary
      // fold, not the intended final grouping for 'unlabeled'.
      source.push(hit)
    } else if (hit.conceptType) {
      let bucket = derivedByType.get(hit.conceptType)
      if (!bucket) {
        bucket = []
        derivedByType.set(hit.conceptType, bucket)
        derivedTypeOrder.push(hit.conceptType)
      }
      bucket.push(hit)
    } else {
      derivedOther.push(hit)
    }
  }

  const groups: HitGroup[] = []
  if (human.length > 0) groups.push({ kind: 'human', hits: human })
  for (const conceptType of derivedTypeOrder) {
    groups.push({ kind: 'derivedType', conceptType, hits: derivedByType.get(conceptType)! })
  }
  if (derivedOther.length > 0) groups.push({ kind: 'derivedOther', hits: derivedOther })
  if (source.length > 0) groups.push({ kind: 'source', hits: source })
  return groups
}
