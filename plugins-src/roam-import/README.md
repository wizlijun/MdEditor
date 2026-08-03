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
3. **CLI incremental sync** — pull *every page* Roam has changed since the
   last run, daily notes and wiki pages alike, from the same running desktop
   app. Same prerequisites and the same merge semantics as (2); see
   [Incremental sync](#incremental-sync) below.

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

## Incremental sync

`notemd roam-day` syncs the day you name. Incremental sync answers the
question you actually have — *what have I changed in Roam since I last
looked?* — and pulls all of it in one action: yesterday's daily note you
finished this morning, the concept page you created last night, the page you
renamed. Daily notes and wiki pages go to their own folders.

Two entry points, one implementation
(`incremental::sync_since`, driven identically by both):

- **The plugin window** — an "Incremental sync" button beside the existing
  "Sync this day", with the last-sync time shown next to it ("Never synced"
  when there is no ledger yet). Same three-state `roam` CLI probe, same
  `ready` bar to clear.
- **The CLI** — `notemd roam-sync`, for a shell or a cron job.

### The ledger: `.notemd/roam-sync.json`

One small JSON file, **inside the vault**, next to the whole-graph import's
own `.notemd/roam-import.json`:

```json
{
  "graph": "bruce",
  "lastSyncedAt": "2026-08-03T11:58:41.185Z",
  "pages": {
    "8IFJWtnad":  { "path": "wikipage/回顾系统.note.md",          "title": "回顾系统" },
    "08-02-2026": { "path": "dailynote/2026/2026-08-02.note.md", "title": "August 2nd, 2026" }
  }
}
```

`lastSyncedAt` is the watermark; `pages` maps each Roam page uid to the file
it landed in, which is what makes a rename in Roam a *move* here rather than
a second file.

It lives in the vault, not in the app's own data directory, **because it
describes vault files and therefore has to travel with them through git**. A
ledger left behind on one machine would claim that some uid lives at a path
that does not exist on the machine reading it. Two machines syncing the same
graph can produce a git conflict on this file; it is small and flat, so
resolve it by hand — and losing it entirely is not a data loss, only a
wider rescan on the next run (re-syncing a page is idempotent).

It is written atomically (temp file → `fsync` → rename) after **every** page,
not once per run, because the host kills the plugin process on deactivate and
on quit. A ledger that is present but unreadable — truncated, or carrying
`<<<<<<<` markers from a git merge — is deliberately **not** treated as "no
ledger": that would silently mean "start from yesterday" and abandon
everything older, permanently. It is reported in `errors`, and a run with a
non-empty `errors` is never presented as clean (the CLI fails it; see
[Exit codes](#exit-codes)).

### Discovery: two queries, and both are necessary

Finding what changed takes the **union of two datalog queries**, merged by
uid, taking the later timestamp of the two. This is the single most
inviting thing in the feature to "simplify", and dropping either half
silently stops syncing a whole class of change:

| dimension | query | catches | **misses** |
|---|---|---|---|
| max `:edit/time` over the page's **blocks** | `[?b :block/page ?p] [?b :edit/time ?t]` | content edits | a page **renamed or created** without any block changing |
| the **page entity**'s own `:edit/time` | `[?p :edit/time ?t]` | renames, new pages | almost every content edit — for a daily note this value is the moment Roam *created* the page |

Measured on a real graph: `08-03-2026`'s last block edit was
`2026-08-03T11:58Z` while its page entity's `:edit/time` was
`2026-08-02T16:00Z` (local midnight, when Roam made the page). In the other
direction, seven wiki pages had page-entity changes at `2026-08-01T16:18Z`
that the block dimension could not see at all. Neither query is a superset of
the other.

Both are filtered **server-side** (`[(> ?t ?since)]`, with `?since` bound
through `--inputs`), so the result sets stay small however far back the
watermark is.

### The watermark, and why it moves a millisecond at a time

Pages are processed in ascending `(edited, uid)` order — ascending because
that is what makes the run resumable, and with `uid` as tiebreaker so a run's
order is reproducible. On a failure the run **stops** and keeps what it has
done; the next run resumes from the watermark. Network loss, Roam quitting,
the plugin process being killed: nothing is skipped, at the cost of
re-fetching a little (which is free — an unchanged page is not even written).

The watermark advances **a whole timestamp at a time, never a page at a
time**, and this is the part not to "fix":

> `edited` is not unique. A bulk edit, a scripted import, or the two
> discovery dimensions coinciding will give two different pages the same
> millisecond `T`. Say P and Q both have `edited == T`. If P's success moved
> the watermark to `T` and Q then failed, the next run asks Roam for
> everything **strictly** after `T` — and that strictness has to stay, or
> every run would re-sync its own last page forever. Q, which was never
> synced, is not in the answer. It is skipped permanently and silently.

So all pages sharing an `edited` are one atomic group, and the watermark may
only cross `T` once every page at `T` has been dealt with. Equivalently: the
persisted watermark is the greatest `edited` strictly below the smallest
`edited` among this run's failures, or the batch maximum when there are none.

Two related rules: the watermark is a page's own `edited`, never `now` (using
`now` would jump over edits made *while the scan was running* and lose them
forever), and it only ever moves **forward** — `--since` re-reads history
without rewinding the frontier, so the next ordinary run does not rescan a
month.

A first run, with no ledger, starts at **local midnight at the start of
yesterday** — not the beginning of time. Pulling the whole graph is the JSON
import's job, not this one's.

### Where pages land

| the page | path | OKF `type` |
|---|---|---|
| uid shaped `MM-DD-YYYY` (a Roam daily page) | `<daily_dir>/<yyyy>/<yyyy-MM-dd>.note.md` | `Daily Note` |
| anything else | `<wiki_dir>/<sanitized title>.note.md` | `Wiki Page` |

`daily_dir` and `wiki_dir` come from note.md's own vault settings; the file
name is sanitized with the same rules as the whole-graph import (illegal
characters → `-`, empty → `untitled`), and a ` (2)` suffix is added only when
another *uid* already holds that path. A page with no blocks at all — Roam
creates one for every `#tag` — is counted as skipped and no file is written.

**A page renamed in Roam has its file moved.** The ledger knows where the uid
used to live, so the sync `rename`s the old file to the new name before
merging into it: the blocks you wrote in that file, your annotations and the
file's git history all follow the rename instead of being stranded in an
orphan while a fresh file appears next to it. Every move is listed in the
report.

Two things it deliberately does not do:

- **`[[wikilink]]`s pointing at the old name break.** An incremental run sees
  only the pages that changed, so it cannot find (let alone rewrite) the
  links elsewhere in the vault. The rename is *reported*, not repaired — fix
  the links yourself, or don't.
- **A file already sitting at the destination is never overwritten.** The old
  file is left where it is and Roam's content is merged into the destination;
  you then have two files for one page, with your local-only blocks in the
  orphan. That is reported in `errors` (so the run is not clean) rather than
  resolved by clobbering a file that may hold writing which cannot be fetched
  again.

### Deletions in Roam are not propagated

Consistent with the daily sync's third merge rule: a block or a page you
delete in Roam **stays in the vault**. A block that was synced once and is
gone from Roam now is kept and counted as `roam_gone_kept`; a page whose uid
Roam no longer answers for is simply skipped, and its `.note.md` is left
alone. Deleting your copy of something is not this plugin's call to make. If
you want it gone from the vault, delete the file (or the block) there.

### CLI usage

```
notemd roam-sync [--since yyyy-MM-dd] [--graph GRAPH] [--dry-run] [--json]
```

- `--since` — backfill from an explicit day instead of the stored watermark,
  read as **local** midnight (the same day boundary `--date`, the daily
  calendar and the first-run default use; reading it as UTC would start the
  scan hours into the morning east of Greenwich and drop that morning's
  edits). It does not rewind the ledger.
- `--graph` — which Roam graph, if the `roam` CLI is connected to more than
  one.
- `--dry-run` — list what a real run would sync, including the renames it
  would perform, and **write nothing at all**: no note, no file move, no
  ledger, no watermark. The report comes back with `"dry_run": true` and
  `"to": null` (a dry run persists no watermark, so it has none to report).
- `--json` — a single JSON envelope on stdout instead of plain text.

```json
{"ok":true,"data":{"from":"2026-08-03T11:58:41.185Z","to":"2026-08-04T09:12:00.000Z","scanned":12,"synced":9,"skipped":2,"failed":1,"renamed":[{"uid":"8IFJWtnad","from":"wikipage/旧名.note.md","to":"wikipage/新名.note.md"}],"errors":["…"],"dry_run":false}}
```

`scanned` may exceed `synced + skipped + failed`: a failure stops the run and
the pages after it are left for the next one. `skipped` counts pages that
needed no change — gone from Roam, blockless, or already byte-for-byte what
Roam holds. `failed` is at most 1.

### Exit codes

Both `roam-day` and `roam-sync` use the same codes, and a **cron job should
check them**:

| code | meaning |
|---|---|
| `0` | success, and the run was clean |
| `1` | an unexpected host-side error before the plugin ran |
| `2` | bad invocation — a required argument was missing |
| `3` | the plugin is disabled, not installed, or the v2 runtime is off |
| `4` | the run failed, **or finished with problems** |
| `127` | no such `notemd` subcommand at all |

Note the second half of `4`. A run that synced every page it found but hit an
unreadable ledger, or refused a rename because a file was already at the
destination, has `failed: 0` and a non-empty `errors` — and it is **not**
clean, because it may be under-scanning. Such a run exits `4` with
`{"ok":false,"error":{"code":"plugin_failed","message":"…"}}`, rather than
reporting success. Everything the plugin itself rejects lands here too,
including no vault configured, the `roam` CLI missing or not connected, and
an invalid `--since` (the plugin validates it, not the argument parser).

**Builds up to and including 6.803.1 exited `0` from every plugin
subcommand**, whatever happened: Tauri's exit path discarded the code the CLI
had computed. A cron job could not tell a failed sync from a good one. Fixed
after 6.803.1 — if you are scripting against an older build, do not trust
`$?` there.

## Known trade-off: `#.rm-hide` / `#.rm-private`

Roam's UI hides blocks tagged `#.rm-hide` from view, and `#.rm-private` is
used the same way for material meant to stay out of sight. Neither tag is
visible to the CLI's `datalog-query`, which has no way to distinguish
"hidden" from "visible" — a tagged block or page is fetched, converted and
merged in like any other, by both the daily sync and the incremental sync.
Neither of them filters those tags. If you rely on them to keep
scratch/meta/private material out of your Roam UI, be aware it will still
show up in the synced `.note.md` files.

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
