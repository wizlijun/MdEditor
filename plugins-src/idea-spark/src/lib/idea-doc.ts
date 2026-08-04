// Builds the on-disk text for an idea's original `.md` file. Purely a thin
// wrapper over the vendored OKF writer (./okf/concept.ts) — no `generated`
// actor, because idea originals are human-authored (§7: human: prefix only
// applies when we'd claim authorship; here we simply don't stamp one).
import { CONCEPT_TYPE, conceptFileText, touchConceptFrontmatter } from './okf/concept'

/** Full idea document text: OKF frontmatter (`type: Idea`, `created`) + body. */
export function buildIdeaDoc(body: string, nowIso: string): string {
  return conceptFileText({ type: CONCEPT_TYPE.idea, created: nowIso }, body)
}

/**
 * Re-saving an idea that is already on disk. The editor only ever holds the
 * *body* (frontmatter is stripped on load), so a plain `buildIdeaDoc` on save
 * would silently drop every frontmatter key this plugin doesn't know about and
 * restamp `created` with the save time. `touchConceptFrontmatter` instead fills
 * in only what is missing — existing keys keep their value and their order —
 * which both preserves the user's/host's keys and heals a frontmatter that lost
 * its mandatory `type` (OKF §4.1 hard constraint).
 *
 * `frontmatter` is the raw YAML between the fences (no fences);
 * `createdFallback` is only used when the block has no `created` yet.
 */
export function rebuildIdeaDoc(frontmatter: string, body: string, createdFallback: string): string {
  const meta = touchConceptFrontmatter(frontmatter, {
    type: CONCEPT_TYPE.idea,
    created: createdFallback,
  })
  return `---\n${meta}\n---\n${body}`
}
