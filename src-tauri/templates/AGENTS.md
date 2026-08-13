---
type: Vault Conventions
title: AGENTS.md
description: Conventions for AI agents working in this vault.
---

# AGENTS.md

Guidance for AI agents working in this vault. This file is the source of
truth; CLAUDE.md is a symlink to this file — edit AGENTS.md only.

(The frontmatter above is not decoration — see "Metadata" below. Every
markdown file in this vault carries one, this file included.)

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

## Metadata: OKF-compatible frontmatter (required)

Every markdown file you create here **must** open with a YAML
frontmatter block, and that block **must** carry a non-empty `type`.
The format is the Open Knowledge Format (OKF) v0.2:
https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md

It is still plain Markdown plus YAML — readable, greppable, diffable.
The metadata is what lets the next agent, or the human two years from
now, see where a file came from and how far to trust it. A file without
it is a wall of text with no provenance.

    ---
    type: Outline Note
    title: Block references address one assertion, not one paragraph
    description: Why a block ref points at a claim.
    tags: [outline, references]
    generated: { by: your-agent/version, at: 2026-08-03T14:22:00Z }
    sources:
      - id: roam-export
        resource: /sync/roam-2026-07-30.md
        title: Roam export, 2026-07-30
    ---

`type` is the **only required key**. Use the value that matches where
the file lives; types are not centrally registered, so if nothing fits,
coin a short one and then use it consistently.

| `type` | for |
|--------|-----|
| `Note` | a plain markdown document |
| `Outline Note` | any `.note.md` (outline or companion note) |
| `Daily Note` | `dailynote/yyyy/yyyy-MM-dd.note.md` |
| `Wiki Page` | `wikipage/<title>.note.md` |

Everything else is optional, but absent metadata means "unknown", not
"fine" — write what you actually know:

- `title`, `description`, `tags`, `resource` — recommended on every
  concept. `title` repeats the filename title; `resource` is a URI for
  the underlying asset, when the note is about one.
- `generated: { by, at }` — who produced the content, and when it last
  changed substantively (not every touch).
- `verified: [{ by, at }]` — confirmation events, appended not
  replaced. A single entry may be written as a bare mapping.
- `sources: [{ resource, id, title, author, last_modified }]` — what
  the content was derived from. `resource` is an absolute URL, a
  vault-absolute path (`/sync/foo.md`), or a relative path. Attribute
  individual claims with markdown footnotes keyed by `sources[].id`.
- `status: draft | stable | deprecated` (absent means `stable`) and
  `stale_after: YYYY-MM-DD` for content with a known shelf life.

Every actor string — `generated.by`, `verified[].by`, and the `by::`
property in the answer protocol below — takes one of three forms:

| form | for | example |
|------|-----|---------|
| `<producer>/<version>` | an agent or tool | `claude-code/opus-5` |
| `human:<id>` | a person | `human:jdoe` |
| `process:<id>` | automation | `process:nightly-sweep` |

**Never sign your own work `human:`.** That prefix is the only thing
marking content a person wrote or confirmed — the same line the editor
draws between `✦` (written by AI) and `●` (thought by you). Mixing the
two destroys the signal.

Rules that hold in both directions:

- **Preserve what you did not write.** When rewriting a file, add
  missing keys only; never drop, reorder or rewrite existing keys,
  including ones you do not recognise.
- **Never reject a document over its metadata.** No frontmatter, an
  unfamiliar `type`, unknown extra keys, a link whose target does not
  exist — read it anyway and leave it alone.
- **`index.md` and `log.md` are reserved names.** Do not write ordinary
  notes under them: `index.md` is a directory index (a list of links
  with descriptions), `log.md` a change log grouped under `YYYY-MM-DD`
  headings, newest first.
- The document-level `status` above (`draft`/`stable`/`deprecated`) is
  **not** the node-level `status::` of the Q&A protocol below
  (`open`/`answered`/`adopted`). Same word, different namespaces; never
  convert one into the other.

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
           by:: your-agent/version

   **The fence must open the bullet.** The backticks are the first
   characters after `- `, and nothing else goes on that line. In
   particular, never hand-write the `✦` sigil: the editor renders it for
   you, and typing it pushes the fence down onto a continuation line
   where the outline parser cannot see it. The body's list items are
   then read as outline nodes, `type:: answer` binds to the wrong
   bullet, and the answer is torn apart the next time the note is saved.

       - ✦                       ← WRONG: fence is no longer on the bullet
         ````markdown
         …

   The opening fence must be longer than the longest run of backticks
   inside the answer (four here, because the body contains a three-tick
   block). Write exactly one answer node per question — answering again
   replaces it. Keep the property lines in the order shown. `by::` is
   an actor string: your producer name and version, never `human:`.
4. Set the question's `status::` to `answered`.

The human reads the answer inline under the annotated paragraph and may
insert it into the document; doing so sets `status:: adopted`.

### The answer node is the only deliverable

Answering produces one bullet and one status change — nothing else. No
explanatory document, no research report, no `wikipage/` entry, no new
file anywhere in the vault; no edit to `index.md` or `log.md`; no second
copy of the answer restated at length in chat.

### How to write the answer

- **Short.** Conclusion first, reasons after. Three to eight lines by
  default.
- **No warm-up, no wind-down.** Do not restate the question, do not set
  the scene, do not open with "let's analyse this", do not close with a
  summary paragraph.
- **Structured past three sentences.** Longer than that, break it into a
  short list or a couple of small headings, so a reader can take the
  conclusion away at a glance.
- **Honest about doubt.** "Not certain — needs checking against X" beats
  an invented certainty. If you do not know, say which part you do not
  know.
- **Sourced in place.** When you lean on something outside the question
  — a URL, or another file in this vault — put the link or the vault
  path next to the claim it supports.

Hard rules: never set `status:: closed` or `status:: adopted` (only the
human closes or adopts), never edit the main `.md`, never modify any
existing bullet that is not your own answer node, never touch any other
part of the outline, never write any file other than the `.note.md` that
holds the question, never put anything — `✦` included — before the
opening fence on the answer bullet.

## Searching this vault

This vault has a local full-text index. Prefer it over a raw `rg` sweep: it is
faster, it knows Chinese word boundaries, and it ranks the notes you have
actually annotated above machine-generated summaries of them.

```
notemd search <query...>            # path:line:text, ranked, exit 1 = no match
notemd search "exact phrase"        # phrase match
notemd search x tag:y type:z        # filters: tag: type: path: ext: after: before: page:[[X]] origin:
notemd search origin:human          # only what a human wrote/signed (vs derived|source|unlabeled)
notemd search origin:unlabeled      # files with no frontmatter and no source-glob match — fix these
notemd search x --json              # adds score, breadcrumb, source_ref, provenance, origin
notemd search x --context 2         # surrounding lines
notemd search x --all               # every hit — default cap is 20 (--limit N adjusts, 0 = no cap)
```

`rg` and `grep` keep working and are never wrong to use — the index is an
accelerator, not a gatekeeper. When a result's `provenance.agent_by` is set, the
text was written by a model: follow its `sources` to the primary document before
relying on it. `origin` classifies a whole file into `human` (you wrote or
signed it) / `derived` (a model generated it) / `source` (raw material a model
still has to read) / `unlabeled` (nobody has claimed it — no frontmatter, no
source-glob match) — filter to `origin:human` to see only what a human
actually judged, or `origin:unlabeled` to find files worth labeling one way or
the other, or read the field in `--json` output to weigh a hit accordingly.
Unlabeled files are ranked lowest by default (×0.3) and can fall out of the
top results entirely, so `origin:unlabeled` is the way to find them anyway.

## House rules

- (Add your own project conventions below.)
