# note.md

[English](README.md) · [简体中文](README.zh-CN.md) · [notemd.net](https://notemd.net)

> **Read what AI writes. Keep what you think. Keep what only *you* can write.**

A markdown reader, editor, and bidirectional-linking notes tool for the
AI-native era. Native macOS app, ~11 MB to download, ~15 MB installed. Your
notes are a folder of plain `.md` files on your disk — forever.

[Download](https://notemd.net/download) · [Plugins](https://plugins.notemd.net) · [Full feature list](docs/FEATURES.md)

---

## 1. The best place to read what your agents wrote

Rich view and source view, one keystroke apart. Import any Notion- or
Typora-style theme. Mermaid, Graphviz, and KaTeX all tuned and lazily loaded.
No bundled Chromium — the whole app installs at ~15 MB.

Highlight a claim, leave a question in the margin, fix the sentence right
where it's wrong.

Claude, Codex, OpenClaw each have their own chat window. None of them is a
place to *read*. This is.

## 2. Everything the last generation got right, built in

Local-first. Git sync. Outliner. `[[wikilinks]]` and backlinks. Wiki pages.
Daily notes. Full-vault search. A plugin system.

Roam Research and Obsidian figured these out. note.md ships them on files:
one plugin imports your entire Roam graph, and an Obsidian vault opens
directly.

## 3. No AI inside. Fully AI-native.

note.md calls no model and sends no request.

Your vault is designed to be the shared, version-controlled context that many
agents and many harnesses work in — Claude Cowork, Claude Code, Codex,
ChatGPT Work, OpenClaw, Hermes — through public conventions (`AGENTS.md`,
block citations, sidecar `.note.md`). A memory system is on the roadmap.
The more you leave in the vault, the better every agent knows you.

Switch AI tools whenever you like. The asset stays yours.

Specifically tuned for building products with Claude Code: writing docs,
reading docs, and reviewing what the AI generated.

## 4. Whatever else you need, grow it yourself

Write a plugin. Wire an OpenClaw cron job. Hang skills off it.

Put a `?` in an annotation and an agent picks it up: it revises the document
asynchronously, fills in the context you asked for, and hands it back for you
to accept — or not.

The rest is yours to discover.

---

## Five convictions

1. **AI text is infinite; your attention isn't — your judgment is the residue.**
   What you leave in the margins is the one thing no model can generate.
2. **Files over app.** Every note is a plain `.md`: git-friendly, greppable,
   readable in fifty years. Indexes are derived; files are the only truth.
3. **Agents are first-class citizens — they suggest, you confirm.**
   [The graph grows only where you confirm it](docs/product-principle-relationships-only-grow-where-confirmed.md);
   note.md never auto-connects your notes or fills the vault with agent slop.
4. **[Your marks belong to the vault, not to a path.](docs/product-principle-mirror-hosted-marks.md)**
   Annotate a file from anywhere and it's mirrored in, so your marks get a
   stable, git-versioned host.
5. **[One vault, many agents — you orchestrate.](docs/product-principle-one-vault-many-agents-you-orchestrate.md)**
   The workers are interchangeable. You hold the pen.

## Strictly OKF v0.2

Conviction 2 needs a format, not just a file extension. note.md follows the
[Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
(OKF) v0.2 — Google Cloud's open spec for knowledge that humans and agents
exchange: plain Markdown, YAML frontmatter, diffable, portable.

- **Everything it writes conforms.** Every document note.md creates opens
  with YAML frontmatter carrying the required `type` — ⌘N notes, `.note.md`
  sidecars, daily notes, wiki pages, Roam and e-book imports, generated
  reports. Existing outline notes get their `type` filled in the next time
  you save one; a plain `.md` you brought from elsewhere is left exactly as
  it is — no app cruft injected into your files, ever.
- **Provenance in the spec's vocabulary.** Source, trust and lifecycle use
  OKF's own fields — `sources`, `generated`, `verified`, `status`,
  `stale_after` — and its actor forms, `<producer>/<version>` for a tool,
  `human:<id>` for a person, `process:<id>` for automation. So "a human
  confirmed this" becomes machine-readable, not a hunch.
- **Everything it reads is tolerated.** A missing optional field, an
  unfamiliar `type`, unknown extra keys, a dead link — none of these ever
  cause a document to be rejected, and keys note.md doesn't understand
  survive a round trip untouched. That's OKF's tolerant conformance, and the
  spec makes it a MUST.
- **Checked, not claimed.** `pnpm okf:lint <dir>` audits any folder against
  the spec's three hard constraints; every path that writes a document is
  tested against it.

Your agents get the same contract: the vault's `AGENTS.md` spells out the OKF
requirement, so anything working in that folder writes conformant files too.
Still landing: the app filling in `generated` / `verified` on your behalf, and
bundle-level export (`index.md`, `log.md`, wikilinks rewritten as OKF links) —
tracked in the [conformance audit](docs/okf-v0.2-conformance-audit.md).
Format details: [`docs/okf-v0.2-format-constraints.md`](docs/okf-v0.2-format-constraints.md).

## Written by AI, answered for by a human

note.md is developed and maintained entirely by AI coding, so releases come
fast. The maintainer is a career software engineer; every change is reviewed,
tested, and smoke-run before it ships.

## Under the hood

Built with [Tauri](https://tauri.app) on
[`@moraya/core`](https://www.npmjs.com/package/@moraya/core): a code-signed,
notarized native macOS `.app` — native Rust binary, native menus / window /
tray — with the editor UI rendered in the system WebView (WKWebView), not a
bundled browser.

The product name is **note.md** (all lowercase — a note that *is* a plain
markdown file). The CLI binary and bundle identifier are `notemd` /
`net.notemd.app`; the legacy `mdedit` symlink still works. You'll still see
`mdeditor` in the source tree (the Rust crate is `mdeditor_lib`). Versions
before v4.8.0 shipped as **M↓**.

## Develop & build

```bash
pnpm install
pnpm tauri dev            # develop
pnpm tauri build          # build, current arch
```

Both architectures (each its own `.app`; universal mode is retired):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target aarch64-apple-darwin
pnpm tauri build --target x86_64-apple-darwin
```

Output: `src-tauri/target/<arch>-apple-darwin/release/bundle/macos/note.md.app`
(or `src-tauri/target/release/…` for the current arch).

## CLI

```bash
notemd search "query" --vault ~/Vault      # full-text search, prints path:line:text
notemd search "query" --json               # hits with source_ref, origin, provenance
notemd search "query" --all                # every hit (default cap: 20; --limit N adjusts)
notemd share draft.md                      # publish a share link, prints URL
notemd share draft.md --json               # structured output
notemd share draft.md --unshare            # remove the share
notemd plugin list                         # all plugins and their status
notemd reading-insights report --vault ~/Vault --date 7d
notemd help                                # full reference
```

Built-in core commands plus anything contributed by *enabled* plugins.
Install from **Help → Install 'notemd' Command in PATH…**.

`notemd search` is the one agents reach for. It is grep-shaped on purpose, so
`rg` habits carry over, and `--json` returns `source_ref` (`path#Lline`)
alongside each hit's provenance — a hit written by a model says so, and can be
followed to the primary document instead of trusted. Filters use the same
grammar as the app's search panel (`tag:` `type:` `path:` `ext:` `after:`
`before:` `page:[[X]]` `origin:`). See `notemd help search`.

## Release (maintainers)

```bash
scripts/release.sh <x.y.z>
```

Tests → version bump → signed per-arch builds → notarize → tag → push →
GitHub Release (two `.dmg`s, two updater tarballs + signatures, and a
`latest.json` manifest driving per-arch auto-update). Requires `APPLE_ID`,
`APPLE_PASSWORD`, `APPLE_TEAM_ID` in `.env.release` and the updater key at
`~/.tauri/mdeditor.key`.

## Docs

- Full feature list: [`docs/FEATURES.md`](docs/FEATURES.md)
- Knowledge-document format (OKF v0.2):
  [`docs/okf-v0.2-format-constraints.md`](docs/okf-v0.2-format-constraints.md)
  · [conformance audit](docs/okf-v0.2-conformance-audit.md)
- Writing plugins: [`docs/plugin-v2-development.md`](docs/plugin-v2-development.md)
- Designs & plans: `docs/superpowers/specs/`, `docs/superpowers/plans/`

## Thanks

To [**Effie**](https://www.effie.pro/) and
[**Hulunote**](https://github.com/hulunote/hulunote) — for their support and
encouragement, and for showing what a distraction-free writing tool and an
open-source bidirectional-linking outliner can be. The bundled **effie** theme
is a nod to the former.

## License

Apache-2.0 (consistent with `@moraya/core`).
