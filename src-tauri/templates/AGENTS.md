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

## House rules

- (Add your own project conventions below.)
