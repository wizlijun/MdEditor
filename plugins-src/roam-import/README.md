# Roam Research Import

Bring [Roam Research](https://roamresearch.com) content into a note.md vault as
plain `.note.md` outline pages. There are two independent paths:

1. **Whole-graph JSON import** — parse a Roam JSON export and write every page
   as a `.note.md` file (wiki pages + daily notes), with a manifest that
   tracks what has already been imported so re-running the import is safe.
2. **CLI daily sync** — pull *one day* of Roam's own daily-note page straight
   from a running Roam Research desktop app (via the
   [`@roam-research/roam-cli`](https://github.com/Roam-Research/roam-tools)
   tool) and merge it into that day's `.note.md`, either from the plugin
   window or from a shell/cron via `notemd roam-day`.

## Prerequisites for the CLI daily sync

- Roam Research **desktop app running** with the graph open.
- The `roam` CLI installed (`npm install -g @roam-research/roam-cli` or
  equivalent) and **connected** once via `roam connect` — this pairs the CLI
  with the running desktop app. The plugin window's "使用 Roam CLI 同步"
  toggle shows the live probe result (`missing` / `not_connected` / `ready`,
  with the CLI's version and the graphs it can see) so you can tell which
  step is missing before syncing.
- A vault configured in note.md (the CLI sync writes nothing without one).

The whole-graph JSON import has no such prerequisite — it only reads a file
you already exported from Roam (Roam → graph menu → Export All → JSON).

## Merge semantics (CLI daily sync)

Every sync of a given day re-merges Roam's current daily page into that day's
existing `.note.md`, block by block, identified by Roam's own block uid
(`id::`). Both import paths therefore write `id::` on **every** Roam block, not
just on `((ref))` targets: a page first written by the JSON import and later
synced by the CLI has to align block-for-block, and a block without an `id::`
matches nothing in that merge — it survives as a "local block" beside Roam's
copy and the page doubles. Three rules, in one sentence each:

- **Roam is authoritative for its own blocks.** If a block's `id::` still
  exists in Roam, the file's copy is refreshed to match Roam's current text
  and position — Roam wins any conflict on a block it still owns.
- **Your handwritten blocks are preserved.** Anything you wrote directly in
  the `.note.md` file (no `id::`, or an `id::` Roam no longer reports) is
  never touched or deleted. Its *position* within its level follows one
  rule: it's placed right after the nearest sibling before it that's already
  been placed in the merged output; if there is no such preceding sibling,
  it goes to the head of its level. One corollary worth knowing: a note you
  left at the *end* of a level can move to the *top* on the next sync if
  every one of that level's Roam siblings turns out to be new (nothing
  before your note anchors it anymore) — your words are never lost, but
  their exact position within the level is not guaranteed to be stable
  across syncs the way Roam's own blocks' positions are.
- **Blocks Roam has since deleted are kept, not deleted.** A block that was
  synced from Roam on an earlier run but no longer exists in Roam's current
  page is left in the file rather than removed — deleting the user's copy of
  something is not this plugin's call to make.

Re-running a sync for the same day with nothing changed on either side is a
true no-op: the file is not written at all — not even its `updated:` — so a
scheduled sync does not dirty the note for vault git-sync or re-trigger
note.md's file watcher on a day where nothing happened.

A day Roam has no page for is not written at all — no empty file is created
just because you asked to sync it.

**Do not leave the day's note open in note.md during an unattended sync.** The
sync reads the file, merges, and writes it back, while the app's outline pane
holds its own in-memory copy of the same file — whichever saves last wins. The
sync will not overwrite a change it did not see (it re-reads the file
immediately before publishing and aborts with
`… changed while this sync was reading it — nothing was written`), so nothing
is lost either way; but a cron job that keeps aborting is a cron job that is
not syncing.

## Known trade-off: `#.rm-hide`

Roam's own UI hides blocks tagged `#.rm-hide` from view, but the CLI's
datalog pull has no way to distinguish "hidden" from "visible" — a
`#.rm-hide`-tagged block is fetched, converted and merged in like any other.
If you rely on `#.rm-hide` to keep scratch/meta blocks out of your Roam UI,
be aware they will still show up in the synced `.note.md`.

## CLI usage

```
notemd roam-day [--date yyyy-MM-dd|today|yesterday] [--graph GRAPH] [--json]
```

- `--date` — which day to sync. Accepts a literal `yyyy-MM-dd`, or the words
  `today`/`yesterday` (evaluated against the machine's local calendar, since
  a daily note is a human's day, not UTC's). Defaults to **yesterday**.
- `--graph` — which Roam graph to read from, if the `roam` CLI is connected
  to more than one. Defaults to the `roam` CLI's own default graph.
- `--json` — emit the result as a single JSON envelope instead of plain text
  (see below).

The command writes (or updates) `<daily_dir>/<yyyy>/<date>.note.md` in the
configured vault and exits 0 on success. With `--json`, stdout is:

```json
{"ok":true,"data":{"date":"2026-08-02","path":"dailynote/2026/2026-08-02.note.md","created":1,"updated":0,"kept_local":1,"roam_gone_kept":0,"found":true}}
```

`found: false` means Roam has no daily page for that date — the file is left
exactly as it was (or, on a first sync, never created). A failure (no vault
configured, the `roam` CLI not found/not connected, an invalid `--date`)
exits **4** and, with `--json`, prints
`{"ok":false,"error":{"code":"plugin_failed","message":"…"}}`; without
`--json` the message goes to stderr instead.

This is the same `sync::sync_requested_day` orchestration the plugin window's
"同步当日" button drives — there is exactly one implementation of "sync one
day," reached from either caller.
