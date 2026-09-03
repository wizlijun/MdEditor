---
type: Vault Conventions
title: AGENTS.md
description: Conventions for AI agents working in this vault.
---

# AGENTS.md

Guidance for AI agents working in this vault. This file is the source of
truth; CLAUDE.md is a symlink to this file — edit AGENTS.md only.

(The frontmatter above is not decoration — see "Metadata" below. Markdown
files in this vault carry it except for the generated `/USER.md` and
`/MEMORY.md` plain-text projections.)

## Vault layout

- `/USER.md` — generated plain-text projection of eligible owner facts. It is
  not the machine-readable owner authority source.
- `/MEMORY.md` — generated plain-text projection of eligible durable facts and
  decisions. It is not a task list, daily log, or authority source.
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
- `inbox/tasks/` — executable tasks for Next. Each task is one
  `YYYY-MM-DD-HHmm-<slug>-task.md` file; see "Inbox tasks" below.
- Meeting transcripts live under the Vault-relative directory configured by
  `/.notemd/meetings.json` (`meetings_root`, default `ssot/meetings`); see
  "Meeting transcripts" below.
- Any other folder — regular markdown documents (`xxx.md`), optionally
  with a companion outline note beside them (see below).

## Meeting transcripts

- Resolve the archive root from `/.notemd/meetings.json` key
  `meetings_root`; if the file or value is missing or invalid, use
  `ssot/meetings`. Never assume a different location.
- Each meeting is `<meetings_root>/<conversation-id>/`. It contains exactly
  one authoritative, speaker-attributed, time-coded transcript:
  `transcript.md` or `transcript.srt`. It may also contain `summary.md`;
  every Hemory import contains `meta.yml`. Audio files such as MP3 or WAV do
  not belong here.
- A Hemory import copies the selected transcript and optional summary bytes
  unchanged. Treat them as source artifacts: do not clean up, reorder, rename,
  add frontmatter to, or otherwise rewrite them.
- Hemory tombstones (`_deleted/`, `_deleted_*`, `.deleted_*`, or
  `meta.json` with `deleted: true`) are never imported. If an imported source
  is deleted later, the archived meeting is retained and reported as missing
  from the source; source deletion never cascades into this Vault.
- When `meta.yml` exists, read `transcript_file` and `summary_file` rather
  than guessing filenames. Hemory imports use only these flat top-level keys:
  `conversation_id`, `created_at`, `end_at`, `title`, `category`,
  `key_topics`, `language`, `source`, `duration_ms`, `speaker_count`,
  `transcript_file`, `summary_file`, `imported_from`, and `updated_at`.
  Their `source` is `hemory_v1.0:<original-value-or-unknown>`.
- `/.notemd/meetings/hemory-import-v1.json`, its lock, and its transaction
  journal are plugin-managed migration state. Do not edit, move, or delete
  them.

## Metadata: OKF-compatible frontmatter (required)

Except for the generated root `/USER.md` and `/MEMORY.md` projections and the
plugin-managed meeting source artifacts described above, every markdown file
you create here **must** open with a YAML frontmatter block, and that block
**must** carry a non-empty `type`. The projections and meeting source artifacts
MUST NOT gain frontmatter during a generic metadata fix.
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
| `Task` | one executable item in `inbox/tasks/*-task.md` |

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
  individual claims with markdown footnotes keyed by `sources[].id`, except in
  the controlled `/USER.md` and `/MEMORY.md` projections. Those two files never
  expose inline citation markers, footnote definitions, or per-entry source
  notes; Claim revisions under `/.notemd/memory/` retain provenance.
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

The reverse duty is note.md's, not yours, but it is worth knowing it holds:
every document a person creates through the app — a new note, a quick note, a
companion `.note.md`, a daily note, a wiki page, an idea, a trace request — is
written with `generated: { by: human:<id>, at }`. So the three states are
distinguishable rather than guessed at: a `human:` actor means a person wrote
it, a `<producer>/<version>` actor means a generator did, and no `generated`
key at all means nobody has claimed it. Do not strip or rewrite a `human:`
stamp you find; it is the one signal that cannot be regenerated.

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

## Shared user model and long-term memory

`/USER.md` and `/MEMORY.md` are generated, read-only plain-text projections.
They are computed from the versioned Claim records under `/.notemd/memory/`,
which are the only authoritative memory data. Do not infer status, provenance,
permission, or certainty from projection text alone.

### Projection contract

- Read `/USER.md` first and `/MEMORY.md` second for private, owner-scoped work.
  `/USER.md` contains projection-eligible facts about the confirmed vault owner;
  `/MEMORY.md` contains other projection-eligible durable facts, constraints,
  decisions, commitments, preferences, and boundaries.
- Each projection has exactly one H1. Content uses one layer of H2 categories
  followed by multi-line fact bullets. It contains descriptions only: no YAML
  frontmatter, IDs, workflow state, priority, polarity, confidence, provenance,
  hashes, citations, source notes, or other machine metadata.
- Resolve owner identity with `notemd memory owner --json`. Never parse owner
  identity from `/USER.md`. If the owner is unknown, conflicting, or inactive,
  do not create an owner Task.
- Before using a fact, obtain its current metadata and context decision with
  `notemd memory context --space <space> --purpose <purpose> --caller <caller>
  --provider <provider> --model <model> --json`.
  Treat withheld, pending, quarantined, contested, stale, or conflict-blocked
  Claims as unavailable. Approval to remember a Claim is not proof that it is
  true and never grants permission for an external action.
- In shared, public, or external contexts, do not load, quote, or inject either
  projection unless the owner and the Claim consent policy authorize that use.

### Mutation contract

- Neither humans nor Agents edit `/USER.md` or `/MEMORY.md` directly. An Agent
  may submit only one atomic, owner-related durable Claim at a time with
  `notemd memory propose`; proposing never activates or approves the Claim.
  Do not store facts whose subject is another person merely because the fact
  appeared in the owner's vault.
- Every proposed Claim must keep claim kind, subject, assertor, event time,
  validity time, epistemic basis, truth status, confidence, salience, polarity,
  sensitivity, risk, context, consent, evidence, lineage, and Agent-use guidance
  as separate typed fields. Unknown values stay unknown; never turn a category,
  producer, approver, source, or transaction timestamp into semantic evidence.
- A deliberate owner click on **Confirm**, **Deny**, **Important**, **Ignore**,
  or **Delete** is the decision itself. Persist the exact hash-bound decision
  immediately and do not ask for a second confirmation. Agent-originated
  recommendations remain pending until an authorized human decides them.
- A direct filesystem edit to either projection is not authoritative. Restore
  the generated projection from v2 control data; never silently adopt drift.
- Concurrent devices append immutable revisions and compare protocol, authority,
  and Claim heads. Stale heads, hash mismatches, divergent parents, unsupported
  schemas, and action-sensitive conflicts fail closed until explicitly resolved.
- Delete creates a tombstone. Purge is a separate, explicitly authorized
  operation and cannot promise removal from Git history or another device.
- Never store credentials, tokens, private keys, authentication material, or
  restricted plaintext in Git-backed memory assets.
- Tasks and reminders remain one file per Task under `/inbox/tasks/`; daily or
  episodic detail remains in `dailynote/`; raw material remains with its source.

## Inbox tasks

Agents may add must-do items, including ones discovered during a daily
summary, directly to Next's inbox. A Task is a clear obligation, concrete next
action, or real deadline. It is not a suggestion, an idea worth exploring, a
topic to remember, or work that is already complete. When in doubt, leave the
item in the summary instead of manufacturing a commitment.

### Ownership gate

Before evaluating ownership, call `notemd memory owner --json`. Only create a
Task when its obligation belongs to the **confirmed vault owner** returned by
that command, whose authority state must be active and conflict-free. The source
must explicitly assign the action to that person,
record that person's own commitment, or state a deadline that person must meet.
Never parse owner identity from `/USER.md`; if the authority reducer cannot
confirm the owner, create no Task.

- Never create a Task for work assigned to another person, even when that work
  affects the owner or the owner's project. Keep it in the summary with the
  actual responsible person.
- A team-level action is not an owner Task unless the source gives the owner a
  specific part. Write only that part, not the whole team's obligation.
- "Wait for Alice to send the contract" is Alice's work, not the owner's Task.
  "Follow up with Alice on Friday" is an owner Task only when the source
  explicitly requires that follow-up.
- Do not rewrite somebody else's obligation as the owner's Task. If ownership
  is inferred, shared, or ambiguous, create no Task and flag the ambiguity in
  the summary instead.

### One Task, one file

- Create one file per independently actionable outcome. Do not bundle unrelated
  actions into one Task merely because they came from the same source.
- Store it as a **direct child** of `inbox/tasks/`; Next does not scan nested
  directories.
- The final filename is
  `inbox/tasks/YYYY-MM-DD-HHmm-<slug>-task.md`, using local time and a short
  title-derived slug. It must end exactly in `-task.md`. On a name collision,
  append `-2`, `-3`, and so on; never overwrite the existing file.

Use this complete shape for an Agent-created Task:

```yaml
---
type: Task
title: Submit the TestFlight build
description: The build exists locally but has not been uploaded and verified.
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
  project: NoteMD
  priority: P1
  due: "2026-09-02"
  contexts: ["@computer"]
  done_when: The build is available and installation is verified
  dedupe_key: daily-summary/v1:2026-09-01:testflight-upload
generated: { by: your-agent/version, at: 2026-09-01T03:20:00Z }
sources:
  - id: daily-note
    resource: /dailynote/2026/2026-09-01.note.md
    title: Daily note for 2026-09-01
---

Confirm the signing environment, upload the build, then install it once.
```

Field rules:

- Required for every Task: `type`, `title`, `created`, `task.version`, and
  `task.id`. `type` must be exactly `Task`; `title` must be a non-empty action;
  `created` must be an RFC 3339 datetime with an explicit timezone;
  `task.version` must be the integer `1`; and `task.id` must be a newly
  generated UUID v4 that is not already used by another Task.
- A Task title is an exception to "Naming a note" below: write a concrete
  verb-led action ("Submit the TestFlight build"), not a claim or topic.
- Required in addition when an Agent creates the Task: `generated.by`,
  `generated.at`, `task.dedupe_key`, and at least one non-empty
  `sources[].resource`. Use UTC `Z` timestamps for both `created` and
  `generated.at`. `generated.by` uses `<producer>/<version>`, never `human:`.
- `task.project` records the Task's source-level project affiliation. Include
  the exact project name whenever the source establishes one; omit it rather
  than guessing when the Task is genuinely unassigned. This is context, not a
  confirmed Next project tag, and it does not mark the Task current.
- Optional: `description`, `task.project`, `task.priority`, `task.due`,
  `task.contexts`, `task.done_when`,
  `sources[].id`, `sources[].title`, and Markdown body text. If present, each
  scalar value must be a non-empty string. `task.priority` must be `P0`, `P1`,
  `P2`, or `P3` (P0 is highest; use P2 when no stronger signal exists).
  `task.contexts` is a non-empty sequence of unique place, tool, or person
  contexts. `task.due` is a real hard calendar deadline and **must be quoted** as
  `"YYYY-MM-DD"` so YAML cannot coerce it into another type. `task.done_when`
  states observable completion evidence, not a vague intention.
- The body may hold concise execution context, source links, or a checklist.
  It does not replace the action-oriented `title` or the provenance in
  `sources`.
- Do not write `status`, `done`, `lane`, or lifecycle events into the Task.
  Inbox / current / waiting / later / completed state belongs only to Next's
  ledger and to the human's actions in Next.

### Deduplication and safe publication

`task.dedupe_key` identifies the logical obligation across Agent reruns, not
this particular file write. Keep it deterministic; a recommended form is
`<producer>/<schema-version>:<period>:<stable-source-key>`. Include an
occurrence date for genuinely recurring work so separate occurrences do not
collapse into one Task.

Before writing, scan all existing direct `inbox/tasks/*-task.md` files:

1. If the same `dedupe_key` already exists for the same logical Task, creation
   is a no-op. Do not create a second file, even if the existing Task has
   already been completed in Next.
2. If the same key exists with materially different content, report a conflict;
   never overwrite, merge, reopen, or silently choose one version.
3. If the newly generated `task.id` already exists, generate another UUID v4
   and check again.

This is a **create-only** protocol. Write and validate a temporary file in
`inbox/tasks/` whose name does not end in `-task.md`, then publish it with a
no-clobber rename. Re-read the published file and verify its path, YAML, UUID,
timestamps, required Agent fields, and exact content. Clean up only the
temporary file if validation or publication fails.
Never modify, move or delete an existing task. Never modify `thinking/next.note.md`;
only the human's action in Next may mark a Task current, waiting, later,
completed, or reopened.

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

## Mermaid diagrams

note.md renders fenced Mermaid with **Mermaid 11.17.2** in rich editing,
preview, sharing, printing and PDF export. Put one complete diagram in each
`mermaid` fence; the first non-comment line selects its grammar:

````markdown
```mermaid
flowchart LR
  source["Source note"] --> check{"Verified?"}
  check -->|yes| stable["Stable note"]
```
````

Grammars available in this build:

- Everyday diagrams: `flowchart`/`graph`, `sequenceDiagram`, `classDiagram`,
  `stateDiagram`/`stateDiagram-v2`, `erDiagram`, `journey`, `gantt`, `pie`,
  `quadrantChart`, `requirementDiagram`, `gitGraph`, `mindmap`, `timeline`,
  `kanban`, and C4 (`C4Context`, `C4Container`, `C4Component`, `C4Dynamic`,
  `C4Deployment`).
- Data and structured diagrams: `xychart`/`xychart-beta`,
  `sankey`/`sankey-beta`, `block`/`block-beta`, `packet`/`packet-beta`,
  `architecture-beta`, `radar-beta`, `treeView-beta`, `eventmodeling`,
  `ishikawa`/`ishikawa-beta`, `venn-beta`, `treemap`, and `wardley-beta`.
- New experimental families: `cynefin-beta`, `swimlane-beta`,
  `railroad-beta`, `railroad-ebnf-beta`, `railroad-abnf-beta`, and
  `railroad-peg-beta`. Use a beta grammar only when it fits the request; do
  not invent syntax from a Mermaid version newer than 11.17.2.

Generation rules for reliable rendering:

- Use short ASCII identifiers (`user`, `api`, `step_1`) and put human text in
  labels. Unicode labels, including CJK and emoji, are supported; quote a label
  when it contains Mermaid punctuation such as brackets, braces, colons,
  semicolons, pipes or arrow-like text.
- Comments start with `%%`, never a single `%` (which is ordinary text in this
  version).
- `classDef`, `class` and `style` are supported. Keep CSS declarations valid
  and braces balanced; unsafe at-rules and malformed CSS are discarded by the
  renderer's sanitizer. Do not depend on external scripts, stylesheets or HTML
  side effects — exported diagrams are standalone SVG.
- Flowcharts support additional shapes and collapsible subgraphs; pie charts
  support donut rendering; XY charts support labels and legends; class diagrams
  use the unified renderer; and C4 labels are pure SVG with long-label wrapping.
- Keep `quadrantChart` axis, quadrant and point labels concise: Unicode works,
  but this version does not auto-wrap those labels or interpret Markdown or
  `<br>` inside them.
- Prefer the simplest grammar that communicates the relationship. If syntax is
  uncertain, generate a small valid diagram instead of guessing a directive.

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
notemd search x --json              # adds score, breadcrumb, source_ref, provenance, origin, attention_minutes
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

### If you can't run `notemd` yourself

Some harnesses can call tools but can't reach a binary on this machine. note.md
serves the same index over MCP for those — register it once, in the client's
own config, and `search` / `vault_info` show up as tools:

```json
{ "mcpServers": { "notemd": { "command": "notemd", "args": ["mcp"] } } }
```

`search` takes only `query`, `limit` (0 = no cap) and `context`. The filters go
inside the query string, exactly as above — one grammar, whether you type it in
a shell or pass it as a tool argument. Hits carry the same fields `--json` does,
so everything above about `origin` and `provenance.agent_by` applies unchanged.

Two fields exist only on this surface, and both are worth reading before you
trust a path:

- **`vault_info.index_state`** — `ready` means answers are real. `opening` means
  wait a few seconds and ask again. `failed` or `idle` means the index isn't
  serving; that is when falling back to `rg` is the right move, not a defeat.
- **`mount.status`** — whether the paths you get back mean anything *to you*.
  `matched`: a folder you have mounted is this vault, so open hits directly.
  `mismatched`: it isn't. Use the returned `text` and `breadcrumb`; resolving
  `/dailynote/2026/x.note.md` against your own mount would open a different file
  that happens to share the name. `unknown`: the client didn't say what it has
  mounted — compare `<your mount>/.notemd/vault-id` against the response's
  `vault_id` before you resolve anything.

A `truncated: true` response means the byte budget cut the list short, not that
the vault holds no more. Narrow the query and ask again.

Calling `vault_info` once at the start of a session costs nothing and is the
cheapest way to learn all of the above. This surface is read-only: it can find
things and tell you where they are, but it will never write to the vault.

## House rules

- (Add your own project conventions below.)
