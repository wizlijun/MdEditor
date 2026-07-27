# AGENTS.md

Guidance for AI agents working in this vault. This file is the source of
truth; CLAUDE.md is a symlink to this file — edit AGENTS.md only.

## Vault layout

- `dailynote/` — daily outline notes, organized as
  `yyyy/yyyy-MM-dd.note.md` (e.g. `2026/2026-07-10.note.md`).
  Monthly and yearly summaries live in the same year folder as
  `yyyy-MM.note.md` and `yyyy.note.md`.
- `wikipage/` — default home of global wikilink pages. Each page is an
  outline note named `title.note.md`, created when a `[[title]]` link is
  first resolved.
- `sync/` — markdown documents copied in from outside the vault (the
  editor's sync-to-vault feature). Each file is a snapshot of an external
  original; edits here do not flow back to the source file.
- `answers/` — long-form answers written by agents to human questions
  (see "Questions & answers" below), named `yyyy-MM-dd-<slug>.md`.
- Any other folder — regular markdown documents (`xxx.md`), optionally
  with a companion outline note beside them (see below).

## The `.note.md` suffix

- A file ending in `.note.md` is an **outline note**: a bullet-list
  outline with per-node metadata, edited in a dedicated outline view.
- **Companion rule:** if `xxx.note.md` sits next to `xxx.md` in the same
  folder, the two are companions — the `.note.md` holds outline
  annotations for the main document. Treat them as a pair:
  - Do not edit, rename, move, or delete one without the other.
  - Do not "fix" the outline structure of a `.note.md` file; its format
    is managed by the editor.
    The only sanctioned write is the Q&A protocol below.

## Questions & answers (`type:: question`)

A human annotation whose text contains `?` (or `？`) is a **question
addressed to you**. In the companion `.note.md` it appears as a child of
an annotation node:

    - the annotated source text
      type:: annotation
      line:: 142
      - why is this claim true?
        type:: question
        status:: open

Sweep protocol — how to answer:

1. Find nodes with `type:: question` and `status:: open` across the
   vault (`grep -rn "status:: open" --include="*.note.md"` works).
2. Read the source context: the parent annotation node's `line::`
   points at the annotated line in the companion main document
   (`xxx.md` beside `xxx.note.md`).
3. Write a short answer as an indented child bullet of the question
   node, prefixed with `✦ ` and followed by two property lines:

       - why is this claim true?
         type:: question
         status:: answered
         - ✦ because …
           answered:: 2026-07-27T14:22:00Z
           by:: your-agent-name

4. A long answer goes to its own file under `answers/`
   (`answers/yyyy-MM-dd-<slug>.md`); keep only a one-line `✦` summary
   plus a link under the question node.
5. Set the question's `status::` to `answered`.

Hard rules: never set `status:: closed` (only the human closes a
question), never edit the main `.md`, never modify any existing bullet
that is not your own `✦` answer, never touch any other part of the outline.

## House rules

- (Add your own project conventions below.)
