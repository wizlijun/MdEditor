// Builds the on-disk text for an idea's original `.md` file. Purely a thin
// wrapper over the vendored OKF writer (./okf/concept.ts) — no `generated`
// actor, because idea originals are human-authored (§7: human: prefix only
// applies when we'd claim authorship; here we simply don't stamp one).
import { CONCEPT_TYPE, conceptFileText } from './okf/concept'

/** Full idea document text: OKF frontmatter (`type: Idea`, `created`) + body. */
export function buildIdeaDoc(body: string, nowIso: string): string {
  return conceptFileText({ type: CONCEPT_TYPE.idea, created: nowIso }, body)
}
