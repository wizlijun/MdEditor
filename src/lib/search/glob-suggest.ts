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
 * chain. Rung 2 IS a strict subset of rung 3 for any vault — same top-level
 * directory, with rung 2 adding a trailing extension constraint rung 3
 * doesn't have — so that pair always orders narrow-to-wide by real match
 * count too. Rung 1 vs rung 2 is the pair that is NOT comparable: rung 1 is
 * the file's own directory with no type filter, rung 2 is the *top*
 * directory with a type filter, and neither is a subset of the other. This
 * is not an edge case — it's the core scenario this feature exists for: a
 * mixed-media import folder like `imports/session1/{a.mp3,b.png,c.docx,
 * d.srt}` alongside `imports/{other-notes.md,logs.txt}` gives rung 1
 * (`imports/session1/**`) 4 files and rung 2 (`imports/**\/*.srt`) 1 file —
 * rung 1 is *wider* there, by real count, in an ordinary transcript-import
 * layout. The candidates are still presented narrow-to-wide by **scope**
 * (each rung is a deliberate, nameable restriction, strictly looser than
 * the last in what it constrains), not sorted by whichever happens to match
 * fewer files in a given vault — C-T11's UI must not promise ascending
 * counts. Design spec §7.1's own worked example (12 / 340 / 1,204 files)
 * happens to have ascending counts too, but that's a property of that one
 * example vault, not a guarantee this function makes.
 *
 * Rung 2 (`top-dir + this extension`) is kept as its own candidate rather
 * than collapsed into a strict `rung2 ⊂ rung3 ⊂ ...` chain — dropping it
 * would lose "the top directory, restricted to this file type", which is
 * usually the single most useful candidate for a transcript-import folder
 * that also holds other file types.
 *
 * **The caller default-selects rung 1.** That default holds regardless of
 * which rung turns out to match fewer files in a given vault (see the
 * `imports/session1` example above, where rung 1 matches more than rung 2)
 * — what makes rung 1 the right default is that it is the most
 * *intentional* scope (exactly the directory the pasted sample lives in),
 * not that it is the smallest number. And the asymmetry in failure modes
 * still favors it either way: writing the pattern too narrow is
 * self-correcting — the user notices results are missing and widens it —
 * whereas writing it too wide fails silently, quietly pulling unrelated
 * files into the index with no symptom for the user to notice at all. When
 * the failure modes are that lopsided, default to the one a human will
 * actually catch.
 *
 * Candidates that collapse to the same string (a single-level path makes
 * rung 1 and rung 3 identical — both reduce to `<dir>/**`) are deduped,
 * keeping the first, narrower occurrence.
 */
export function suggestGlobs(samplePath: string): GlobCandidate[] {
  const normalized = samplePath.replace(/\\/g, '/').replace(/^\/+/, '')
  // Drop empty segments (collapses `//`, and a leading/trailing `/`) AND `.`
  // segments (a leading `./`, or an internal `/./`, both ordinary artifacts
  // of `find .` and several path-copy tools). Left unstripped, a leading
  // `./` would survive into every candidate as a literal path segment —
  // vault-relative paths the backend walks never have one, so the resulting
  // patterns would match nothing, including the very file just pasted.
  const segments = normalized.split('/').filter((s) => s.length > 0 && s !== '.')
  if (segments.length === 0) return []

  // NOTE: a trailing slash (`suggestGlobs('ebook/三体/')`) is not detected as
  // "this is a directory, not a file" — the empty-segment filter above drops
  // it, so the last real segment (`三体`) is read as an extension-less
  // filename, producing `['ebook/**']` rather than a `三体`-anchored ladder.
  // Harmless (still non-empty, still self-matching, never mismatches its own
  // sample), just not the ladder a directory paste would deserve — out of
  // scope here since every caller today pastes a file path, not a directory.
  const dirs = segments.slice(0, -1)
  const filename = segments[segments.length - 1]
  const dotIndex = filename.lastIndexOf('.')
  // `dotIndex > 0` (not `>= 0`) so a dotfile like `.gitignore` is treated as
  // extension-less rather than yielding the empty-body extension `''`.
  //
  // Lower-cased (final fix wave, Blocker 3): the sample path is pasted from a
  // real file, and transcripts routinely arrive from external tooling as
  // `B.SRT`. Copying that case verbatim produced `media/**/*.SRT`, which
  // reads to a human as "the uppercase ones only" even though the matcher
  // now folds an extension filter's case (`searchidx/src/globs.rs`) — so the
  // generated pattern would be a needlessly surprising, non-canonical
  // spelling of the same set. `toLowerCase()` here and
  // `to_ascii_lowercase()` in the Rust `parse` normalize to the same
  // canonical form from both ends. This is canonicalization only: the
  // correctness fix is in the matcher, because it also has to handle a
  // hand-typed `*.SRT` and, more importantly, a lower-case pattern meeting
  // upper-case FILES, which no amount of normalizing here can reach.
  const ext = dotIndex > 0 ? filename.slice(dotIndex).toLowerCase() : ''

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
