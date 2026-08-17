# Changelog

What changed in each release, from the point of view of someone using note.md.
For the full commit history, see the git log.

中文版:[CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)

## Unreleased

## v6.817.3 — 2026-08-17

### Fixed

- **Sidecar notes never leak into the source folder anymore.** For a file synced into the vault via the "Sync to Vault" menu, annotating it (e.g. asking a question) and then saving used to silently create a second `.note.md` next to the original file outside the vault — a stale orphan that agents and later edits never updated, while the real note lived beside the vault copy. The legacy bidirectional note mirroring behind this is now removed entirely: sidecar notes live only in the vault, no sync path writes the source folder anymore. A pre-existing note next to a source file is still adopted into the vault on first sync (copied, source left untouched). The folder view's "has note" badge now lights up only when a synced file actually has a note, instead of for every shared/synced file.
- **Decision Log no longer hangs on "Loading…" when two decisions share an id.** (Plugin marketplace: Decision Log 1.1.3.) Agent-written day files once assigned the same id to two different decisions; after both ended up archived, rendering the board crashed on the duplicate key and the window sat on the loading screen forever. The board now renders every entry even with duplicate ids.

## v6.817.2 — 2026-08-17

### Fixed

- **Attention-weighted search had no effect at all for anyone.** The ingest assumed one level of nesting more than the day files actually have, so every file failed to parse and was skipped in silence — the log said `attention ingest: 0 files`, Settings showed `0 / 9581`, and ranking counted no attention whatsoever. It now parses the real on-disk shape; on a real vault that means 60 day files and 537 documents with attention data.

## v6.817.1 — 2026-08-17

> **The search index rebuilds itself once, the first time you open a vault after upgrading** (about ten seconds; the search panel shows "building" meanwhile). The index is disposable derived data — the rebuild does not touch a single file in your vault.

### Added

- **Search now counts the time you have spent reading.** Time you put into a document (reading plus editing, editing weighted 1.5×) lifts it in search results, decaying with a 30-day half-life — what you read through last month gives way to what you are working on this week. The data comes from Reading Insights, which was already collecting it; nothing new is recorded. Documents you have never opened are **not penalised**, merely unboosted: the summary an agent generated this morning is exactly the thing you are searching for.
- **Long documents you have actually read no longer get buried.** A long note you spent three hours in, but which mentions your query word only once, used to be cut by the relevance limit before scoring ever happened. It now gets a reserved slot into the ranking stage.
- **Settings ▸ Search** gained a "Files with attention data" row, showing how much of your vault this data covers.
- **`notemd search --json` gained an `attention_minutes` field** — decayed minutes of your own attention on that document, so an agent can explain why the order is what it is. The plain `path:line:text` output is unchanged, to the character.
- **New `notemd doctor` command** — one command that checks environment, config and vault, search index, plugins, and network endpoints, reporting what is wrong in each rather than a blanket "not working".

### Changed

- Search result order will shift. That is the direct consequence of the above: documents you have invested time in move up.
- The strength of the attention boost is configurable: `searchWeights.attention` in `.notemd/settings.json` (default `0.4`; `0` turns it off entirely). A change takes effect on your next search — no need to reopen the vault.

### Fixed

- When a file outside your vault is mirrored into it, the reading time you spent on the original is now credited to the vault-side mirror instead of being lost.
