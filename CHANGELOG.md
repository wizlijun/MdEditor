# Changelog

What changed in each release, from the point of view of someone using note.md.
For the full commit history, see the git log.

中文版:[CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)

## Unreleased

### Changed

- **Position Log is now Location Log.** The plugin name and its Capture menu item now use one concise name in every supported language. Existing installs upgrade in place because the internal plugin ID remains `notemd.pos-log`.

### Fixed

- **The native Plugins menu now displays `Import & Export` with its ampersand intact.** The menu backend's mnemonic marker is escaped before building the category submenu.

## v6.828.3 — 2026-08-28

### Changed

- **Plugin categories now use concise, task-oriented names and one home per plugin.** The Plugins menu and marketplaces now use Agents, Capture, Reading, Thinking, Import & Export, and Editing. Existing category keys remain readable during upgrades, while unknown third-party values still fall back to Other.

## v6.828.2 — 2026-08-28

### Changed

- **Plugins are now organized by capability instead of one long menu.** The Plugins menu and both marketplace views share five stable groups: Agents, Capture & Import, Thinking & Review, Publish & Export, and Editor Extensions. Claude, Codex and DeepSeek now sit together under Agents; unknown third-party group values remain visible under Other.

### Fixed

- **Idea Spark now lists only files explicitly named `*.idea.md`.** Ordinary Markdown, proof documents, directories and case-mismatched suffixes in the idea folder no longer appear as ideas. New and renamed ideas keep the `.idea.md` suffix, and an orphaned proof file can no longer make a fresh idea look already argued.

## v6.828.1 — 2026-08-28

### Fixed

- **Agent concurrency now lives where it belongs: inside each Agent plugin.** Claude Agent, Codex Agent and DeepSeek Agent each have a Settings entry at the bottom of their own window. The page keeps the same 1–5 “Maximum concurrent AI reads” control and preserves values already chosen. The three plugin tabs have been removed from note.md's global Settings, and plugins can read or write only their own settings scope.

## v6.827.2 — 2026-08-27

### Added

- **AI book reading can now use several agents at once without overrunning any provider.** Claude Agent, Codex Agent and DeepSeek Agent each get a “Maximum concurrent AI reads” setting from 1 to 5 (default 1). Ebook Import keeps a separate FIFO lane for each provider, so Claude and DeepSeek can work at the same time while additional Claude jobs wait behind Claude's own limit. Changing the setting takes effect while the queue is running; lowering it lets active reads finish, and the same book is still never dispatched twice.
- **Codex is now a first-class note.md agent.** Install the Codex Agent plugin and it drives your locally authenticated Codex CLI directly: the agent window streams progress, keeps run history, supports cancellation, and exposes the same workflow through `notemd codex-agent`. Built-in tasks share note.md's task directory without overwriting another provider's instructions, while each run uses Codex's native read-only, workspace-write, or danger-full-access sandbox.
- **Ebook Import now shows your whole library, and lets the AI read any of it again.** The window used to know only about the files you dropped into it this session — a book imported last month was out of reach, and re-reading one meant importing it a second time. Below the import queue there is now a Library list of every book already in the vault, searchable by title, showing when each was imported and the date of its latest AI digest. Any row opens `book.md`, opens the digest, or starts a fresh read with the agent of your choice. A re-read overwrites that day's digest and leaves earlier ones alone, so the history stays on disk. Asking for the same book from both lists no longer starts two runs — the second one joins the first instead of racing it to write the same file.
- **The AI reading prompt is a file you can edit.** Settings has a new row for `.notemd/agent-tasks/ai-read-ebook/CLAUDE.md` — the instructions the agent follows when it reads a book. Click it and it opens in the main editor like any other note. Change what the digest should emphasise, and every read after that follows your version. (The file appears once Claude Agent has run at least once; until then the row says so.)
- **Ebook imports now keep their join time as data.** Every imported book directory gets a `meta.yml` with an explicit `added_at` timestamp in RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`), instead of leaving the time to be guessed later from its month folder or mutable file timestamps. The Library is sorted by this timestamp, newest first.

## v6.827.1 — 2026-08-27

### Fixed

- **DeepSeek Agent can now deliver ebook summaries outside its task workspace.** AI reading keeps the source book and destination summary inside a run-scoped workspace for the harness, then copies only the declared result back into the Vault. The agent no longer reaches the final write with a path that its sandbox rejects, and concurrent reads cannot mistake another run's output for their own.
- **Codex Agent now uses the Vault's real Codex model and no longer turns a completed long run into an error.** Codex starts in the Vault root, resolves the same effective model as a CLI session there, and writes that model into AI provenance instead of the `codex/default` placeholder. A successful `turn.completed` also remains successful when the CLI needs more than three seconds to shut down after delivering the file.
- **Plugin windows got the IME fix too.** Idea Spark, Trace Source and Power Mode write into the same rich editor the main window uses, and it carried the same defect v6.824.3 fixed: backspacing an IME candidate away to nothing ate the character in front of it, and the Enter that confirmed a candidate could act twice. They pick this up from the app — no plugin update to install.

## v6.824.3 — 2026-08-24

### Fixed

- **Deleting the *last* character of an IME pre-edit string no longer eats the character in front of it.** v6.824.2 fixed Backspace during composition, but not the one keystroke that both deletes the last pre-edit character *and* ends the composition: some webviews announce the composition as over *before* they deliver that key, so by the time it arrived it no longer looked like an IME key and the editor treated it as an ordinary edit. A composition is now considered to own that closing keystroke too, so backspacing a candidate away to nothing leaves the text before it alone. Every editor applies the same rule — outline, source mode, rich mode, the annotation popup and recalled-block edits — so the Enter that confirms a candidate is no longer read as "commit this line" either. Rich mode needed more than looking away: the editor engine already recognises that keystroke but only declines to act on it, leaving the browser's own delete to run, so it is now cancelled outright.

## v6.824.2 — 2026-08-24

### Fixed

- **Backspace with an IME candidate window open no longer eats an extra character.** Typing Japanese, Chinese or Korean in the outline and pressing Backspace to correct the pre-edit string deleted a character in the IME *and* merged the line into the one above it — one keystroke handled twice, once by the input method and once by us. Keys pressed mid-composition now belong to the input method throughout the app: the outline's Backspace/Enter/arrow-key line commands, the source editor's auto-pairing, the rich editor's slash menu, the annotation popup's Enter-to-save and Enter in a recalled-block edit all hold off until the candidate window closes. Once the text is committed nothing changes.
- **Cmd+A in rich mode selects the whole document again.** It used to grab only part of it — just the folded frontmatter block, or, with the caret inside a code block, only that block — while right-clicking ▸ Select All worked fine on the same file. The shortcut was the one path that never used the editor's real select-all: macOS was swallowing the chord as an Edit-menu key equivalent, and whatever slipped through hit the underlying editor's own crude `Mod-a`. All three entry points (keyboard, right-click, Edit menu) now apply the same selection. Side effect worth knowing: Cmd+A no longer shows a shortcut next to Edit ▸ Select All, and it no longer gets stolen from the search box or other text fields — it selects whatever actually has focus.

## v6.824.1 — 2026-08-24

### Added

- **`notemd .` and `notemd xxx.md` open things in the app.** No subcommand to remember: hand `notemd` a path and it lands in the window — a file opens as a tab, a directory becomes the folder view's root. Paths are resolved against the shell's working directory, so relative ones work; if the app is already running the path goes to that window instead of starting a second one, and the command returns to your prompt without waiting. A missing file says so (`cannot open 'x.md': No such file or directory`) instead of "unknown command", and a command name still wins over a same-named file — write `./search` to open the file. See `notemd help open`.

## v6.820.1 — 2026-08-20

### Fixed

- **`notemd reading-insights` no longer reports nothing at all.** It was looking for `.mdeditor/analytics/` in your vault while the app has long been writing to `.notemd/analytics/` — the directory simply wasn't there, so the report came back empty without complaint. A spot the product rename missed.

### Added

- **Trace Source grew an inbox and settings.** (Marketplace: Trace Source 1.1.0.) Reports now land in `inbox/traces/` by default (changeable in the window's settings), named `<date>-<time>-source-trace.md` with full-text materials in a same-named folder beside each report. The new inbox panel — same shape as Idea Spark's — lists past reports newest first; click one to read it in the main editor, right-click to open or delete it (materials included). While a trace runs, the bar shows live progress and the inbox refreshes itself the moment the report lands. The delegation prompt is editable from settings, and the task template migrates itself once — no manual deleting this time.
- **A trace can no longer be lost.** (Marketplace: Trace Source 1.2.0.) The moment you delegate, your ask is saved to disk — `…-source-trace/00-request.md` beside where the report will land — *before* the agent is involved, and the run itself is registered so a closed or refreshed window picks it right back up: still running shows ⧖, finished shows the report, and a run that produced nothing shows ✗ instead of vanishing. A failed row's right-click menu loads your request back into the editor for another go. If saving the request fails, the delegation refuses to start rather than silently proceeding without a safety net.
- **MCP server**: note.md can now serve your vault's search to agents. Register
  `{"command": "notemd", "args": ["mcp"]}` in Claude Code / Cowork / Codex and the agent
  gets Chinese segmentation, relevance ranking and origin weighting instead of falling back
  to grep. Read-only; no network port is opened.

## v6.818.8 — 2026-08-18

### Changed

- **Tracing a passage to its source is now its own plugin.** Install "Trace Source" (溯源) from the marketplace and the right-click 「溯源」 item appears; without it, the item stays out of the menu instead of opening a window that isn't there. Right-clicking a selection opens a dedicated composer prefilled with the quoted passage — add scope notes ("only YouTube and arxiv") and hand it to an agent; the report lands in `traces/` and a notification tells you when it's ready. A menu entry under Plugins opens a blank composer for text copied from elsewhere.
- **The `/溯源` slash command is gone, and the delegation text speaks your language.** The dedicated window IS the trace mode, so there is no command word to type — and no Chinese protocol labels in an English or Japanese UI: the machine-readable lines are now the language-neutral `Source-Doc:` / `Output:`. Idea Spark goes back to being purely about capturing and arguing ideas; the slash-command layer it carried for tracing is removed. (If you used tracing before this release, delete `.notemd/agent-tasks/trace-source/` once so the new template can be seeded.)

## v6.818.7 — 2026-08-18

### Fixed

- **"DeepSeek Harness — found, but it will not start: env: node: No such file or directory" is gone.** Both agents are launched through a small Node script, and an app started from the Dock inherits a much smaller `PATH` than a terminal does — one with no `node` in it. The run itself always compensated for that; the check that reports which harness you have did not, so a perfectly working install was reported as broken, and reporting it broken disabled the Run button. Nothing to do on your side.

## v6.818.6 — 2026-08-18

### Changed

- **DeepSeek Agent now uses the DeepSeek Harness you installed.** It looks for `dsh` on your machine, puts the ACP bridge into a `notemd` profile of its own the first time you run something (`dsh plugin --profile notemd add @deepseek-ai/dsh-acp`), and launches through it. Everything the harness gives you — its tools, skills, subagents, web search — comes along, because the profile does the composing instead of a hand-written file that could only ever mount a subset. Install it with `npm i -g @deepseek-ai/dsh`; the plugin does the rest.
- **The vault's harness file is now an overlay, not a whole composition.** `<vault>/.notemd/dsh/cordis.patch.yml` says only what note.md changes — where writes are allowed, which model, and silencing one thing that would corrupt the protocol stream — layered on top of your profile. It is still plain text in your vault, still yours to edit. The previous `cordis.yml` is removed on first run if you never edited it.

### Fixed

- **"Cannot find package 'tsx'" is gone.** The plugin used to fall back to driving a DeepSeek Harness *source checkout* through its development toolchain, which needed a pinned package manager, a dev dependency, and a 900-package install — none of which anyone has, and all of which failed in ways that had nothing to do with your vault. That path is removed. The setup hint now names the one install that works.

## v6.818.5 — 2026-08-18

### Fixed

- **A failing DeepSeek Agent run no longer shows an empty window.** The real incident: with a stale harness checkout, pnpm silently reinstalled dependencies for minutes before starting the server, then failed on a network hiccup — and all of it went to stdout, so the run record (and the window) had not a single line to show. Both ends are fixed: runs no longer trigger a silent install (broken dependencies now fail within seconds, with a readable "run pnpm install in your checkout" error), and when a run fails, the launcher's last stdout output is kept in the run record so the window can say why.

### Added

- **Idea Spark: the prompt you delegate with is now yours to edit.** Settings has a new "Agent prompts" section listing "Argue an idea" and every `/directive` you have; clicking one opens it in the normal Markdown editor. The prompt has always been an ordinary file in your vault (`.notemd/agent-tasks/<task>/CLAUDE.md`) — this just puts a door on it. Edits apply to the next delegation, and the plugin never overwrites what you wrote.

### Fixed

- **Picking a different agent in a sidecar note now actually takes.** The choice was held in a value the interface did not watch, so the tick and the "by …" label kept showing the previous agent — it looked like selecting DeepSeek had failed. The choice now applies immediately, is remembered across restarts like the other surfaces, and a run in progress keeps reporting through the agent that started it instead of following a later change.

## v6.818.4 — 2026-08-18

### Fixed

- **The agent picker's menu is no longer cut off by the window.** It was positioned inside the panel it lives in, so any scrolling container around it — the sidecar note panel, the ebook queue — clipped it before it ever reached a window edge. It now opens in front of everything and chooses its own direction: downward by default, upward when the button sits near the bottom, and shifted sideways rather than off the edge in a narrow panel. It follows its button while you scroll instead of drifting away from it.

### Added

- **Trace to source: find where a passage really came from.** Select text in the editor → right-click "Trace source", or type `/trace` in Idea Spark, and an agent hunts down the original source across YouTube, paper archives and western tech blogs (scope is yours to narrow in the delegation text), downloads the subtitles or article body, and writes a summary with backlinks into your vault's `traces/` folder — one click back to the document you asked from, and relative links onward into the captured full-text material. A notification opens the summary when the run finishes. YouTube subtitles use a local `yt-dlp` (without it, the report degrades to link + description and says so honestly).
- **Slash directives in Idea Spark.** Any agent task template under `.notemd/agent-tasks/` that declares a `directive` becomes a `/command` on the input surface — write your own template, get your own directive; tracing is just the first one.

## v6.818.3 — 2026-08-18

### Changed

- **One way to choose which agent runs something, everywhere.** Every button that hands work to an agent now carries the same control beside it — `[ Answer ] by Claude ▾` — in the sidecar note panel, in Idea Spark, and on the ebook queue's AI-read action. The menu lists each installed agent with its harness version and the model it will use, and ticks the one in use, so you can tell what you are about to spend tokens on before you click. An agent whose harness cannot start is shown with the reason rather than silently offered as if it worked.
- **Each place remembers its own agent.** Proving an idea with one agent while books are read by another is a normal thing to want, so the choice is per surface rather than one global setting. An ebook queued for a particular agent still runs on that agent even if you change the picker while it waits.
- **The agent windows state which harness they are.** Claude Agent and DeepSeek Agent show `by Claude · 2.1.233` beside their Run button — as a statement, not a menu: that window is that agent, so offering to switch inside it would only be able to send the work elsewhere.

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
