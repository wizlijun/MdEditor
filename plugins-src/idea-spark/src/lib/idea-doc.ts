// Builds the on-disk text for an idea's original `.md` file. Purely a thin
// wrapper over the vendored OKF writer (./okf/concept.ts) — no `generated`
// actor, because idea originals are human-authored (§7: human: prefix only
// applies when we'd claim authorship; here we simply don't stamp one).
import { parse as parseYaml } from 'yaml'
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
 *
 * Healing is not always possible. `touchConceptFrontmatter` refuses to touch a
 * frontmatter that is not a YAML mapping (a sequence, a bare scalar) and
 * returns it verbatim; it leaves an existing but empty/non-string `type` alone;
 * and on syntactically broken YAML it *throws* (`yaml` will not stringify a
 * Document that carries parse errors). The first two would ship a document with
 * no usable `type`, violating OKF §4.1 — the one hard constraint every `.md`
 * this project writes must satisfy — and the third would blow up the save. In
 * all three cases we build a fresh compliant document instead and demote the
 * unusable block into the body, so the user's bytes survive (visibly, editable)
 * rather than being silently dropped.
 */
export function rebuildIdeaDoc(frontmatter: string, body: string, createdFallback: string): string {
  let meta: string | null
  try {
    meta = touchConceptFrontmatter(frontmatter, {
      type: CONCEPT_TYPE.idea,
      created: createdFallback,
    })
  } catch {
    meta = null
  }
  if (meta === null || !hasUsableType(meta)) {
    const salvaged = frontmatter.trim()
    return buildIdeaDoc(salvaged ? `${salvaged}\n\n${body}` : body, createdFallback)
  }
  return `---\n${meta}\n---\n${body}`
}

/**
 * Same predicate the repo's OKF linter applies (`scripts/okf-lint-core.mjs`:
 * mapping + `typeof type === 'string'` + non-blank), so "passes this check"
 * and "passes `pnpm okf:lint`" cannot drift apart.
 */
function hasUsableType(frontmatter: string): boolean {
  let parsed: unknown
  try {
    parsed = parseYaml(frontmatter)
  } catch {
    return false
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return false
  const type = (parsed as Record<string, unknown>).type
  return typeof type === 'string' && type.trim() !== ''
}
