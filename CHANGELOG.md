# Changelog

What changed in each release, from the point of view of someone using note.md.
For the full commit history, see the git log.

中文版:[CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)

## Unreleased

### Added

- **Trace to source: find where a passage really came from.** Select text in the editor → right-click "Trace source", or type `/trace` in Idea Spark, and an agent hunts down the original source across YouTube, paper archives and western tech blogs (scope is yours to narrow in the delegation text), downloads the subtitles or article body, and writes a summary with backlinks into your vault's `traces/` folder — one click back to the document you asked from, and relative links onward into the captured full-text material. A notification opens the summary when the run finishes. YouTube subtitles use a local `yt-dlp` (without it, the report degrades to link + description and says so honestly).
- **Slash directives in Idea Spark.** Any agent task template under `.notemd/agent-tasks/` that declares a `directive` becomes a `/command` on the input surface — write your own template, get your own directive; tracing is just the first one.

## v6.818.2 — 2026-08-18

### Fixed

- **The agent windows no longer sit on "Checking the harness…" forever.** The banner asked the plugin for its harness over a channel the plugin only answered on for menu and relay calls, so the window's question was never answered — and a failed check rendered exactly like one still in progress, so the spinner never stopped. Both are fixed: the question is answered on the window's own channel, and a check that fails now says so.
- **"Not installed" is no longer shown for a harness that is installed.** A harness that is present but cannot start (a version pin, a missing dependency) now says "found, but it will not start" plus the reason and where it was found, instead of sending you off to install something you already have.
- **DeepSeek Harness works from a local checkout again.** The harness repository pins an exact pnpm version, and corepack refuses to run rather than switching — so on any machine whose pnpm differs, the agent reported itself unavailable. The launcher now downgrades that pin to a warning. It also reads the harness version from the checkout instead of starting a server to ask, which is both instant and accurate.
- **The setup hint no longer recommends something that cannot work.** `npm i -g @deepseek-ai/dsh-acp-demo` was the advice; that package requires two dependencies (`dsh-workspace-context`, `dsh-bash-env`) that were never published, so the install either refuses or produces a binary that will not start. The hint now points at the local-checkout route, which does work.

## v6.818.1 — 2026-08-18

### Fixed

- **The Agent area under a sidecar note is back.** v6.817.4 made it check whether an agent plugin was installed by reading a manifest field the host never sends to the front-end, so the answer was always "none" and the whole section vanished. The check now uses a flag the host computes and projects.
- **A run says which agent performed it.** Both agent plugins share one task directory and one run history, so a Claude run appeared in the DeepSeek Agent window looking exactly like a DeepSeek run — which is how an expired *Claude* login got read as a DeepSeek failure. Every run now records its agent, history rows from the other agent are labelled, and each window's environment warning only reports its own harness's problems. (Runs recorded before this update carry no agent and are attributed to neither.)

### Added

- **Pick which agent answers.** With both agents installed, the Agent area under a sidecar note gets a picker showing each one's harness, version and model — "Claude Code · 2.1.233" — so you know what you are about to spend tokens on before you click.
- **Each agent window states its environment up front.** A banner above the task list: the harness, its version, the model a run uses when the task pins none, and where the executable came from. If the harness is present but cannot start, it says so with the reason instead of reporting itself ready.
- **An expired login is reported as an expired login.** When the newest run failed on something environmental — expired credentials, a missing API key, a rate limit — the window says so, rather than leaving it to look like the task was at fault.

## v6.817.4 — 2026-08-17

### Added

- **A second agent, and a way to choose between them.** New plugin **DeepSeek Agent** (marketplace) runs [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) against your vault over the Agent Client Protocol, alongside the existing Claude Agent. Both read the same task templates in `.notemd/agent-tasks/`, write the same run records, and answer the same questions in your sidecar notes — a task is a job description, not a binding to one model. Which one serves the agent slot is now a setting (`agentDefaultProvider` in `<vault>/.notemd/settings.json`); with only Claude Agent installed, nothing changes.
- **Task permissions are a file you can read.** DeepSeek Agent tasks carry a `policy.json` naming the sandbox the run is confined to (`read-only` / `workspace-write` / `danger-full-access`) and what to answer if the agent asks for more. The mode is enforced by the harness's own sandbox, not by the prompt, and the task list shows it before you press Run. A `policy.json` that will not parse stops the run rather than falling back to defaults.
- **The harness composition lives in your vault.** `<vault>/.notemd/dsh/cordis.yml` — plain text, in git, editable by hand, and pointed elsewhere with the `dsh_config` setting if you want your own.

### Notes

- DeepSeek Agent is **experimental**: it depends on developer-preview releases of DeepSeek Harness, which promise breaking changes. It needs `npm i -g @deepseek-ai/dsh-acp-demo` (or a `deepseek-harness` checkout) plus a `DEEPSEEK_API_KEY`. The protocol carries committed assistant text only — no live tool activity, and no session resume — so a DeepSeek run's log is quieter than a Claude run's. That is the protocol, not a missing feature.

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
