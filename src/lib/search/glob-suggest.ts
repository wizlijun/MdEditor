// "Generate from a sample path" (task C-T10, design spec §7.1). Pure
// function, no Svelte import, no I/O — the settings page (C-T11) pastes a
// vault-relative path here, gets back an ordered candidate list, and fills
// in each candidate's match count itself by calling the backend
// `notemd_search_glob_matches` command.
//
// Match counts are deliberately NOT computed here. A pattern decides whether
// `.srt`/`.txt` files get indexed at all (§3/§7.1 of the design spec) — they
// are not in the index yet, so counting from the index would always
// undercount, and undercounting is the wrong direction to err in: it nudges
// the user toward a *broader* pattern than they need. Only a real walk of
// the vault on disk sees the truth, and only the backend can do that
// cheaply.
export interface GlobCandidate {
  /** A glob pattern in this project's deliberately small dialect: `**`
   *  (cross-level), `*` (single level), and literal segments — see
   *  `docs/superpowers/specs/2026-08-12-source-globs-and-transcript-indexing-design.md`
   *  §3.1. No brace expansion, no regex. */
  pattern: string
}

/**
 * Turns a pasted sample path (vault-relative, e.g. `ebook/三体/book.md`) into
 * a small ladder of glob-pattern candidates, ordered **narrow to wide**:
 *
 *   1. the file's immediate directory, every file under it
 *      (`ebook/三体/**`)
 *   2. the top-level directory, restricted to this file's extension
 *      (`ebook/**\/*.md`)
 *   3. the top-level directory, every file under it
 *      (`ebook/**`)
 *
 * This is a fixed three-rung ladder, not a formal narrow-is-a-subset-of-wide
 * chain — rung 1 and rung 2 aren't directly comparable (one trades directory
 * breadth for a file-type filter) and in an unusual layout rung 2 could in
 * principle match more files than rung 3's own directory alone would if that
 * directory were smaller than expected. In the layouts this feature targets
 * (a source-material folder holding one kind of import) the ladder reads
 * narrow-to-wide in practice, and design spec §7.1's own worked example
 * (12 / 340 / 1,204 files) is exactly this shape.
 *
 * **The caller default-selects rung 1, the narrowest.** That default is a
 * deliberate asymmetry, not a coin flip: writing the pattern too narrow is
 * self-correcting — the user notices results are missing and widens it —
 * whereas writing it too wide fails silently, quietly pulling thousands of
 * unrelated files into the index with no symptom for the user to notice at
 * all. When the failure modes are that lopsided, default to the one a human
 * will actually catch.
 *
 * Candidates that collapse to the same string (a single-level path makes
 * rung 1 and rung 3 identical — both reduce to `<dir>/**`) are deduped,
 * keeping the first, narrower occurrence.
 */
export function suggestGlobs(samplePath: string): GlobCandidate[] {
  const normalized = samplePath.replace(/\\/g, '/').replace(/^\/+/, '')
  const segments = normalized.split('/').filter((s) => s.length > 0)
  if (segments.length === 0) return []

  const dirs = segments.slice(0, -1)
  const filename = segments[segments.length - 1]
  const dotIndex = filename.lastIndexOf('.')
  // `dotIndex > 0` (not `>= 0`) so a dotfile like `.gitignore` is treated as
  // extension-less rather than yielding the empty-body extension `''`.
  const ext = dotIndex > 0 ? filename.slice(dotIndex) : ''

  const patterns: string[] = []
  const push = (p: string) => {
    if (!patterns.includes(p)) patterns.push(p)
  }

  if (dirs.length > 0) {
    push(`${dirs.join('/')}/**`)
    if (ext) push(`${dirs[0]}/**/*${ext}`)
    push(`${dirs[0]}/**`)
  } else {
    // Root-level file: there is no directory segment to anchor rungs 1/3 on,
    // so the ladder is purely extension scope — this file type at the root
    // only, widening to this file type anywhere in the vault.
    if (ext) {
      push(`*${ext}`)
      push(`**/*${ext}`)
    } else {
      push(filename)
      push('**')
    }
  }

  return patterns.map((pattern) => ({ pattern }))
}
