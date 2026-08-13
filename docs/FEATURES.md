# note.md — full feature list

[English](FEATURES.md) · [简体中文](FEATURES.zh-CN.md) · [← README](../README.md)

Everything note.md does today, in detail. The README keeps the pitch; this file
keeps the receipts.

## The notes layer

The AI-native notes system, rolling out incrementally:

- [x] **Sidecar notes** — highlights and comments made while reading
      `xxx.md` are saved to a companion `xxx.note.md`. The source stays
      pristine and regenerable; your judgment becomes permanent, searchable
      data. A `.note.md` with no sibling source file is a standalone note.
- [x] **Outline editor** — every `.note.md` opens in a Roam-style outline
      view (never the plain markdown editor); outlines persist as nested
      markdown lists, so the files stay readable everywhere.
- [x] **Daily notes** — a dedicated Daily Notes window with an infinite
      lazy-loading feed of `dailynote/yyyy/yyyy-MM-dd.note.md`, one keystroke
      or tray click away; `[[yyyy-MM-dd]]` is the canonical date link and
      `[[page]]` links open inline.
- [x] **Roam import** — one-shot converter (a built-in plugin) from a Roam
      Research JSON export, with date-page rewriting to `[[yyyy-MM-dd]]` and a
      broken-link report.
- [x] **Annotation Q&A loop** — an annotation containing `?` becomes a question
      for your agents; the `.note.md` carries the state machine, an external
      agent sweeps it, and answers come back as `type:: answer` nodes you can
      accept into the document. Agents never write into the source `.md`.
- [x] **Wiki pages** — standalone outline notes under a configurable
      `wikipage/` folder, one `[[title]]` namespace across the whole vault.
      Search knows about them: the page a `[[…]]` would create is pinned to
      the top when you type its name exactly.
- [x] **Global index** — full-vault instant search, ranked by provenance,
      rebuilt from files at any time (the index is derived data; the files
      are the only source of truth). Query grammar: `tag:` `type:` `path:`
      `ext:` `after:` `before:` `page:[[X]]`
      `origin:human|derived|source|unlabeled`, and quoted phrases. Raw
      material (`.srt`/`.vtt`/`.txt` transcripts) is indexed inside folders
      you designate. Renames and moves are recognised by content hash, so
      renaming a directory updates paths instead of rebuilding every file.
      Headless too — see `notemd search` under *Built for agents*.
      (Backlinks and linked references already work across `.note.md`.)
- [ ] **Vault MCP server** — expose `vault_search` / `vault_read` /
      `vault_annotate` so any agent (Claude Cowork, Claude Code, Codex,
      ChatGPT Work, OpenClaw, Hermes, …) can work your vault, with note.md as
      one client among many.

## Reading & annotating

- **Rich reading view** — KaTeX math, Mermaid diagrams, Graphviz (` ```dot ` /
  ` ```graphviz `), highlight.js code blocks; HTML files open in a sandboxed
  iframe preview; ~36 code file types render as syntax-highlighted blocks;
  images open as preview tabs. Renderers load on demand, so a document without
  diagrams costs nothing.
- **Highlight mark** (`^^text^^` or `==text==`) — yellow highlight in both
  modes; `Cmd+H` in source view wraps the selection.
- **Inline annotations** — CriticMarkup-based comments and questions anchored
  in the text, mirrored into the sidecar outline, with `✦` for what AI wrote
  and `●` for what you thought.
- **Block IDs (mdblock)** — every top-level block (paragraph, heading, code
  block, list, table, …) gets a stable `b-xxxxxx` id. Cite any passage from
  anywhere with `((path/to/file.md#b-xxxxxx))` — sub-page granularity for
  humans *and* agents. Ids are edit-resilient (content MinHash + five-pass
  merge); block metadata lives in a central cache, never beside your files.
  Click a gutter marker to copy a citation; `Cmd+Enter` follows one.
- **Reading Insights** — per-document reading / editing engagement
  stored in your vault; turn any date range into a markdown digest from the
  CLI or **View → Reading Insights**.
- **Attachment & video cards** — links to documents, audio, and video render
  as chips / cards; YouTube and Bilibili URLs fetch their titles and render
  as branded play cards.

## Writing & editing

- **Source / rich toggle** (`Cmd+/`) — plain textarea ↔ WYSIWYG, per tab.
- **Slash menu** (`/` on an empty line) and **block shortcuts**
  (`Cmd+1–6` headings, `Cmd+Shift+K` code, `Cmd+Shift+M` math,
  `Cmd+Shift+T` table, `Cmd+Opt+U/O/X` lists, …).
- **Live-preview markers** — typing `**`, `` ` ``, `==`, … stays literal
  until you ask for a mark; existing marks render but reveal their source on
  the caret's line.
- **Wikilinks** — `[[note]]` renders as a link; click to open (or create)
  `note.md` beside the current file; `[[note|alias]]` shows the alias.
- **Task checkboxes**, **bare-URL autolink**, **collapsible + inline-editable
  YAML frontmatter panel**, **line-break fidelity** across export / share.
- **Paste anything** — screenshots land in `{docname}_files/` with relative
  links; files paste as attachment links; images get a click-to-resize
  toolbar (25 / 50 / 75 / 100 %).
- **Context menu** — a full custom right-click menu in both source and rich
  modes.
- **CSV spreadsheet editor** — `.csv` opens as a live grid with formulas
  (`=SUM(A1:A3)`, cross-cell refs), row/column ops, dark-mode themes; a
  `/spreadsheet` slash command embeds a grid inside markdown.
- **Find & Replace** (`Cmd+F` / `Cmd+H`) with regex, whole-word, and
  case-sensitive options, in both modes.
- **New file** (`Cmd+N`) with a random writing prompt, body pre-selected.

## Your files & vault

- **Folder View** — a live directory tree sidebar with recursive regex
  filtering and *Reveal in Finder*; global sort, per-folder pinning, and view
  modes (all / files / with-notes / markdown-by-H1 / notes).
- **Switchable side panels** — a registry of left/right sidebars with a
  title-bar switcher.
- **External change detection** — clean tabs reload silently; dirty tabs get
  a conflict banner (reload / overwrite / recreate on delete). Never silent
  data loss.
- **Sync to Vault** — copy any file into your git-synced vault with
  date-prefixed naming, source ↔ copy mapping, and conflict-aware refresh.
  Annotating a file outside the vault mirrors it in, so your marks always have
  a stable, git-versioned host.
- **Large-file gate** — files above a configurable threshold stay in the
  working tree instead of entering a vault commit; the tray shows the state.
- **Tabs** with dirty indicators and drag-to-reorder; **auto-save** (opt-in);
  **recent files**; Finder double-click / drag-to-open.

## Built for agents

- **Block citations** — `((file#b-xxxxxx))` gives agents a stable way to
  quote and follow passages across the vault.
- **`AGENTS.md` conventions** — the vault's rules live in plain text at its
  root, which every CLI agent already reads.
- **`notemd search`** — retrieval for agents, grep-shaped: default output is
  `path:line:text`, one hit per line, so `rg`/`grep` habits keep working.
  `--json` adds `source_ref` (`path#Lline`), `origin`, and provenance
  (`agent_by`, `human_verified`) — a hit written by a model says so, and an
  agent can follow it to the primary document instead of trusting it. Exit
  codes distinguish "no hits" (1) from "no vault" (2), and retrieval never
  hard-fails: an unusable index or an over-budget freshness check degrades to
  a direct file scan with one line on stderr. Default cap is 20 hits;
  `--limit N` adjusts it and `--all` returns everything.
- **`notemd` CLI** — drive plugin features without the GUI:
  `notemd share draft.md` publishes a share link; `--json` for structured
  output; `notemd reading-insights report` writes engagement digests.
  Install from **Help → Install 'notemd' Command in PATH…**.
- **MCP endpoint** — the share Worker exposes MCP so agents can publish
  documents on your behalf.
- **Plugin system (v2)** — out-of-process plugins over stdin/stdout JSON *plus*
  isolated-webview UI plugins, with declarative manifests (menus, context
  menus, settings panels, sidebars, tray items, CLI subcommands) and
  capability-gated host actions; dormant until invoked. Browse and install from
  the in-app marketplace ([plugins.notemd.net](https://plugins.notemd.net)):
  **Roam Import**, **Base** (Obsidian `.base` tables), **Weekly Review**
  (year-calendar review), **Decision Log**, **OpenClaw Chat**, md→PDF, and more.
  Writing your own: [`plugin-v2-development.md`](plugin-v2-development.md).

## Share & export

- **Share** — `Cmd+Shift+L` publishes the current file as a
  self-contained page on your own Cloudflare Worker: KaTeX, Mermaid SVG,
  syntax highlighting, light + dark, mobile-ready. Update in place, unshare
  anytime; image-heavy docs spill to R2. See `worker/README.md` for
  deployment.
- **PDF export** (`Cmd+Shift+E`) — clean A4 PDF with math, diagrams, and
  highlighted code, rendered by an offscreen WKWebView (no headless
  Chromium).
- **Image upload** — `Cmd+Shift+L` on an image tab uploads to R2 and copies
  the public URL.

## The app

- **Trilingual UI** — English, 简体中文, 日本語 — covering every dialog, the
  native macOS menu bar (system items included), the tray, and plugin
  strings; switch live in Preferences, no restart.
- **Typora-compatible themes** — import any Typora theme `.zip`; pick
  separate light / dark themes that follow macOS Appearance. Ships with
  **default** (GitHub-style) and **effie** (mint-paper, LXGW WenKai).
- **Menu-bar tray**, Typora-style notification bar, full-UI zoom
  (`Cmd+=` / `Cmd+-` / `Cmd+0`).
- **Apple Silicon & Intel** `.dmg`s with per-arch auto-update.
