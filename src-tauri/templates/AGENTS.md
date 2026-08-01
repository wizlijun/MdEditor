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

## Naming a note

A note's filename **is** its title: dated notes are
`YYYY-MM-DD-<title>.md`, wikilink pages are `<title>.note.md`. The title
is the only thing grep, semantic search and other agents see first, so
write for that: a reader who sees nothing but the title decides whether
to open the note — and, if they never open it, has still got the point.

Write the title in the language of the note's own body. Length is one
claim's worth: roughly 6–15 words, or 12–30 characters in Chinese and
Japanese. Longer usually means two claims — split the note instead of
abbreviating the title.

1. **A claim, not a topic.** Subject, verb, assertion — something a
   reader could disagree with. Not "On block references" but "Block
   references exist to make a single assertion addressable".
2. **Compress the content, not the genre.** Delete genre words (notes,
   summary, analysis, thoughts, 笔记, 総まとめ, …) and spend the room on
   the conclusion instead.
3. **Self-contained.** Understandable without the folder, the date, the
   parent note, or the conversation it came from. No back-references
   ("it", "this approach", "the above").
4. **Keep the searchable entities.** People, products, mechanisms and
   parameters spelled out in full, not private abbreviations — grep
   matches literals.
5. **Carry the qualifier.** If the claim holds only under conditions,
   say so: "under X, A beats B", never an unconditional slogan.
   A dropped condition is the most common way a title lies.
6. **No hype.** Ultimate, must-read, definitive, 震撼, 彻底搞懂 — cut.
7. **Never invent certainty.** If you are not sure what the note's core
   claim is, write a descriptive title and say it is descriptive. A
   confident-sounding but wrong title is worse than a dull one.

Three checks; a title ships only if it passes all three:

- **Search** — two years from now, among fifty search results, would the
  title alone make you open this one?
- **Disagreement** — could someone who read the whole note say "I don't
  agree with that title"? If nobody possibly could, it is a description,
  not a claim. Rewrite.
- **Deletion** — delete the body, keep the title: does the reader still
  have the single most important conclusion?

Two exceptions, to be declared rather than forced into a claim:

- Two equally important claims that cannot be merged →
  `Not one claim: <A> / <B>`, and let the human decide whether to split
  the note.
- Pure material (transcript, clipping, raw record) that makes no claim →
  `Material: <source> · <scope>`. Do not invent a conclusion for it.

Mechanics: the title goes into the filename verbatim — non-ASCII is kept
as is, whitespace becomes `-`, and only `/ \ : * ? " < > |` are replaced
with `-`. Keep the H1 in the file identical to the title in its name.

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
3. Write the answer as a single child bullet of the question node, with
   the whole answer body wrapped in a markdown code fence. The fence
   makes arbitrary markdown (lists, `key::`-looking lines, blank lines,
   nested code blocks) opaque to the outline parser, so an answer can be
   as long and as richly formatted as it needs to be:

       - why is this claim true?
         type:: question
         status:: answered
         - ````markdown
           Because …

           - a list item is fine

           ```python
           nested_code = "is fine too"
           ```
           ````
           type:: answer
           answered:: 2026-07-28T14:22:00Z
           by:: your-agent-name

   The opening fence must be longer than the longest run of backticks
   inside the answer (four here, because the body contains a three-tick
   block). Write exactly one answer node per question — answering again
   replaces it. Keep the property lines in the order shown.
4. Set the question's `status::` to `answered`.

The human reads the answer inline under the annotated paragraph and may
insert it into the document; doing so sets `status:: adopted`.

Hard rules: never set `status:: closed` or `status:: adopted` (only the
human closes or adopts), never edit the main `.md`, never modify any
existing bullet that is not your own answer node, never touch any other
part of the outline.

## House rules

- (Add your own project conventions below.)
