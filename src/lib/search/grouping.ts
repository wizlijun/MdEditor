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

export type HitGroupKind = 'human' | 'derivedType' | 'derivedOther' | 'source' | 'unlabeled'

/** One file's hits inside a group. The panel renders this collapsed by
 *  default — a query that matches a long note twenty times used to spend
 *  twenty rows repeating the same path. */
export interface FileGroup {
  /** Vault-relative path — the grouping key, and the hover title. */
  path: string
  /** Absolute path, handed to `openFile`. */
  absPath: string
  /** Basename, the only part shown; the sidebar is too narrow for the rest. */
  name: string
  hits: SearchHit[]
}

export interface HitGroup {
  kind: HitGroupKind
  /** Only set for `kind === 'derivedType'` — the raw `concept_type` string
   *  (e.g. `'Book Summary'`), used verbatim as the group's label. Concept
   *  types are an open, plugin-extensible vocabulary (`CONCEPT_TYPE` in
   *  `src/lib/okf/concept.ts`), not a fixed enum, so this is never
   *  translated — a plugin can introduce a new type without touching i18n. */
  conceptType?: string
  files: FileGroup[]
  /** Hits across all of `files` — the count shown on the group header. It is
   *  hits, not files: "3" next to 「人写的」 means three matches. */
  hitCount: number
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
 *   source           (origin = 'source', the pole for "raw material")
 *   unlabeled        (origin = 'unlabeled' — nobody has claimed this file
 *                      yet, C-T2's honest fourth tier. Given its own group,
 *                      distinct from both poles: see the history note below
 *                      on why folding it into either pole was rejected.)
 *
 * A group that would be empty is omitted entirely, not rendered empty — so
 * the group count is exactly `(human present ? 1 : 0) + (distinct derived
 * types present) + (untyped derived present ? 1 : 0) + (source present ? 1
 * : 0) + (unlabeled present ? 1 : 0)`, which is "more than four" as soon as
 * two or more derived types show up in one result set (by design — see the
 * module doc comment).
 *
 * History (C-T10): `origin = 'unlabeled'` (2026-08-12 design, C-T2 — spec §3
 * rule 6′) went through two wrong homes before landing here. Pre-C-T2,
 * frontmatter-less `.md` hit no branch below and fell through to
 * `derivedOther` — rendered under the *AI-produced* heading, which is a
 * strictly stronger false claim than "raw material" (an unlabeled file might
 * be the user's own unsigned writing; it is definitely not evidence an agent
 * produced it). C-T2/C-T9's interim then folded it into `source` instead —
 * closer, but still a claim the backend never made: `Unlabeled` and `Source`
 * are different rows in §9's "known signal" table. Neither claim is made
 * now: `unlabeled` is its own group, so a hit in it asserts nothing about
 * who wrote it or whether it's raw material — only that nobody has said yet.
 *
 * Within each group, hits are then bucketed by file. Files are ordered by
 * where each one *first* appears — since `hits` arrives score-sorted, that is
 * the file holding the group's best hit. Within a file, hits keep the relative
 * order they arrived in. This function never re-sorts by score; that already
 * happened upstream in `searchidx::query::finish`.
 *
 * File bucketing is per group, not global: a file with both a `human` hit and
 * a `source` hit appears once under each. Merging them would mean picking one
 * pole for a file that genuinely sits in both, which is exactly the
 * distinction the grouping exists to show.
 */
export function groupHits(hits: SearchHit[]): HitGroup[] {
  const human: SearchHit[] = []
  const source: SearchHit[] = []
  const unlabeled: SearchHit[] = []
  const derivedOther: SearchHit[] = []
  const derivedTypeOrder: string[] = []
  const derivedByType = new Map<string, SearchHit[]>()

  for (const hit of hits) {
    if (hit.origin === 'human') {
      human.push(hit)
    } else if (hit.origin === 'source') {
      source.push(hit)
    } else if (hit.origin === 'unlabeled') {
      unlabeled.push(hit)
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
  if (human.length > 0) groups.push(makeGroup('human', human))
  for (const conceptType of derivedTypeOrder) {
    groups.push(makeGroup('derivedType', derivedByType.get(conceptType)!, conceptType))
  }
  if (derivedOther.length > 0) groups.push(makeGroup('derivedOther', derivedOther))
  if (source.length > 0) groups.push(makeGroup('source', source))
  if (unlabeled.length > 0) groups.push(makeGroup('unlabeled', unlabeled))
  return groups
}

function makeGroup(kind: HitGroupKind, hits: SearchHit[], conceptType?: string): HitGroup {
  const group: HitGroup = { kind, files: byFile(hits), hitCount: hits.length }
  if (conceptType !== undefined) group.conceptType = conceptType
  return group
}

/** Buckets by `path`, preserving first-appearance order of files and arrival
 *  order of hits within each. */
function byFile(hits: SearchHit[]): FileGroup[] {
  const order: string[] = []
  const byPath = new Map<string, FileGroup>()
  for (const hit of hits) {
    let file = byPath.get(hit.path)
    if (!file) {
      file = { path: hit.path, absPath: hit.absPath, name: basename(hit.path), hits: [] }
      byPath.set(hit.path, file)
      order.push(hit.path)
    }
    file.hits.push(hit)
  }
  return order.map((p) => byPath.get(p)!)
}

/** `path` is always `/`-separated — `searchidx::norm::rel_path` rebuilds it
 *  that way on every platform, so there is no `\` case to handle. */
function basename(path: string): string {
  const cut = path.lastIndexOf('/')
  return cut < 0 ? path : path.slice(cut + 1)
}
