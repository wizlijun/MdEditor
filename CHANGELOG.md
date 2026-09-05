# Changelog

What changed in each release, from the point of view of someone using note.md.
For the full commit history, see the git log.

中文版:[CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)

## Unreleased

## v6.906.1 — 2026-09-06

### Changed

- **All 16 production plugins receive a coordinated release.** OpenClaw joins the marketplace for the first time; the developer fixture is excluded. Update note.md first, then update plugins and restart. Memory 2.5.2 requires note.md 6.906.1 or newer for the completed document editor; back up collaborative documents before upgrading, and restore matching data and software together if rolling back.
- **Memory upgrade note:** the next successful save upgrades shared-document state to v6. Before upgrading, save or export unsaved drafts and back up both the Markdown document and `plugin_data/notemd.memory/` in the app data directory. Older plugins cannot read v6; downgrading only the plugin is not a rollback.
- **Settings and plugin windows share a clearer, keyboard-friendly interface.** Settings keep navigation and actions visible in narrow windows. Plugin windows use readable supporting text, consistent light/dark colors, visible focus and shared popup menus while retaining their individual workflows. Settings and confirmation dialogs preserve in-progress work and explain failed saves; Agent chat retains unsent drafts when reconnecting.

### Fixed

- **The collaborative Memory document supports whole-document editing.** Cross-block selection, cut/paste, clearing the document, trailing blank paragraphs and hard breaks now persist correctly, and saving no longer interrupts typing or IME input. Formatting, lists, links, images and tables have visible controls; unsaved drafts can be recovered, compared and retried. Block-aware undo checks its target before restoring text, including after an external move. The updated Memory plugin and host Editor Kit must be deployed together.

## v6.905.6 — 2026-09-05

### Changed

- **Smart Lookup puts the results first.** Matching notes and the first original-text preview appear directly, with just one Open original action and a More menu for copying references, quick answers and in-depth research. The window stays a stable size, supports keyboard navigation and narrow layouts, and shows a scrollable activity history with an explicit Stop action.
- **Time-sensitive questions are narrowed before retrieval.** The fast planning step separates document-date constraints from search topics, using a frozen local date and time zone. Validated boundaries apply to every search query instead of treating phrases such as “last week” as keywords. Update Claude Agent to 1.0.30, Codex Agent to 1.0.17 or DeepSeek Agent to 1.1.16 for the matching planning instructions.

### Fixed

- **Interrupted work no longer replaces newer results or answers.** Stopping, changing the question or selecting different notes invalidates late responses. Failures preserve available matches and show concise, actionable messages without exposing raw service errors; copying research instructions never starts an Agent by accident.

## v6.905.5 — 2026-09-05

### Changed

- **The table of contents uses a cleaner current-position highlight.** The blue highlight remains easy to spot without the extra dark strip on its left edge.

## v6.905.4 — 2026-09-05

### Changed

- **The table of contents now follows your reading position.** While it is open, scrolling a Markdown article in Rich or Source mode highlights the corresponding heading, including when multiple headings share the same title.

## v6.905.3 — 2026-09-05

### Added

- **A read-only table of contents makes long Markdown articles faster to navigate.** Open the new outline-style view beside Sidecar Notes and History to preview the current document's heading hierarchy—including unsaved edits—and jump directly to any section, even when heading titles repeat.

## v6.905.2 — 2026-09-05

### Changed

- **Infinite Canvas controls are easier to scan and stay close to the work.** A centered icon dock now groups creation, navigation, clipboard and history actions, while selection-aware toolbars expose the relevant node, multi-selection, group and connection controls beside the canvas content. Refined cards, group labels, handles and selection states improve visual feedback, and multiple selected nodes can receive one color in a single action.

## v6.905.1 — 2026-09-05

### Added

- **Infinite Canvas gains a fuller editing toolset.** Switch between select, pan, freeform lasso and frame drawing; align, distribute or separate overlapping selections with smart guides; resize multiple cards together; and fit groups around their contents. Connections choose sensible facing sides and their labels edit in place. Touch drawing yields to navigation when a second finger arrives, new and pasted cards follow the last pointer position, and view locking, simplified low-zoom rendering and indexed geometry keep larger boards easier to navigate.

## v6.904.8 — 2026-09-05

### Fixed

- **The governed Memory document now supports normal Markdown editing.** Enter and Shift+Enter, multi-paragraph paste, text insertion and deletion, and undo/redo work inside one stable knowledge block; continuous typing is coalesced before durable save without losing IME input or remapping an acknowledged editor state. Edits that cross governed block boundaries still stop safely instead of guessing identity. Requires Memory 2.5.1.

## v6.904.7 — 2026-09-04

### Added

- **A MEMORY-first governed coauthoring preview.** Memory 2.5.0 can create and locally edit a background document with stable block identities; content, proposals, exact-revision assessments and audit state reopen from one durable commit. Agent suggestions remain pending until a human accepts them, and stale results never overwrite concurrent edits. Claude Agent 1.0.29, Codex Agent 1.0.16 and DeepSeek Agent 1.1.15 run the shared review task inside real input-only isolation.

### Security

- **Governed coauthoring no longer trusts Agent task files in the Vault as its isolation boundary.** All three providers compile in the same task protocol, and Memory enables the action only for a healthy provider advertising that exact task, complete machine results and input-only isolation; older providers fail closed.

## v6.904.6 — 2026-09-04

### Fixed

- **Infinite Canvas controls now perform the actions they show.** Groups can be created around a selection, moved with all fully contained cards—including nested or overlapping selected groups—resized, renamed, styled and ungrouped without deleting their contents. Edge reconnect handles, labels, arrow ends and colors now write back to `.canvas`; undo/redo reacts immediately, keyboard movement persists, and desktop/iOS clipboard fallback and touch targets are reliable.

## v6.904.5 — 2026-09-04

### Added

- **Infinite Canvas adds an Obsidian-compatible visual workspace.** Create and open standard `.canvas` files, arrange text, file/image, link and group cards, connect and label them, and use pan/zoom, multiselect, resize, copy/paste and undo/redo. Canvas documents use guarded atomic autosave and external-change handling, preserve unknown JSON Canvas fields for round trips, render Markdown cards safely with one active editor, and load the Svelte Flow surface only when a canvas tab is opened. Desktop file drops and iOS Open In/export-copy flows use bounded local-resource access without embedding remote webpages.

## v6.904.4 — 2026-09-04

### Changed

- **YAML frontmatter now reads like a metadata card instead of a spreadsheet.** The rich editor and shared pages use a quiet, rounded property panel with compact key/value rows and pill-style scalar lists. Wikilinks, Markdown links and bare web addresses are visually recognizable and clickable in reading mode, while editing still preserves their complete source text.

- **Smart Lookup now narrows time-sensitive questions before searching.** Its single fast Planner first estimates a conservative document-date window for requests about current status, recent progress or the latest work cycle, then chooses search terms inside that range. Explicit `after`/`before` constraints remain authoritative, timeless or genuinely ambiguous questions stay unbounded, and the effective intersected range is now carried consistently into Agent handoff. Claude Agent 1.0.28, Codex Agent 1.0.15 and DeepSeek Agent 1.1.14 include the matching planning contract.

## v6.904.3 — 2026-09-04

### Fixed

- **Owner profile facts are projected to `USER.md` again.** Identity, preferences, work style and boundaries use the `user` projection, while decisions, commitments, practices and other durable context remain in `MEMORY.md`. Both files stay disposable, read-only views grouped by Scope and Role; `.notemd/memory` remains the sole authority.

## v6.904.2 — 2026-09-04

### Fixed

- **Smart Lookup no longer cancels a healthy fast Planner before its first result arrives.** The default planning window now reflects real harness startup time and remains configurable with a hard limit; existing 8-second defaults migrate automatically. If Codex's background model catalog refresh is slow, the explicit fast model stays available, ordinary local matches remain visible, and internal provider diagnostics no longer replace the actual timeout or cancellation status.

## v6.904.1 — 2026-09-04

### Added

- **Memory 2.4.3 lets an external Agent install a validated Role/Scope Registry without hand-entering every item.** `notemd memory context-registry replace --file ... --request-id ...` sends a complete JSON or YAML candidate through the same exact-head, hash-chained Host transaction as the Memory UI and takes effect immediately. Stable request IDs can be retried safely, while Claim approval and reassignment apply remain restricted to the trusted UI.

## v6.903.6 — 2026-09-03

### Changed

- **Smart Search & Answers is now Smart Lookup: a fast path from a natural-language request to useful Vault results.** Typing keeps a zero-token preview; Return uses one fast Agent planning pass to separate dates, paths, tags, source types and sorting from real search terms, then the trusted host runs typed search and renders explainable result cards. It no longer automatically tunes, loads Memory, generates an answer, archives feedback or writes a document.

- **Deeper work is explicit and stays with a full Agent.** A compact “Quick summary” action can summarize only a few complete line-level matches under a hard budget and labels the result as based on current matches. “Continue with Agent” hands off the question, typed constraints and Vault-relative references to Claude, Codex or DeepSeek, which re-searches through `notemd` and requests Memory context under its own policy instead of receiving a copied Vault payload.

- **Smart Lookup behavior is configurable without weakening its safety limits.** Settings cover the planning provider, wait time, result/group limits, optional deep retry for zero results, summary budget/style and default handoff Agent. Claude Agent 1.0.27, Codex Agent 1.0.13 and DeepSeek Agent 1.1.13 add the shared input-only `search-summary` task, read-only `vault-research` handoff, idempotent invocation recovery and cancellation support.

### Fixed

- **Long documents and sections can no longer stop the new flow with `invalid source line range`.** File- and section-level matches remain honest navigation results instead of being clipped into misleading answer evidence; Quick summary accepts only complete, current line blocks and skips stale or oversized sources. The retired answer command keeps a bounded compatibility guard for one release, while invalid paths, reversed ranges and changed content still fail closed.

## v6.903.5 — 2026-09-03

### Changed

- **Smart Search now shows a live, scrolling work log while it understands and answers a question.** Safe status lines expose the current planning, search, tuning, evidence, memory and answer stages without leaking provider internals, and remain visible after either success or failure. If answer generation fails, a dedicated retry reuses the already frozen evidence instead of repeating the search.

### Fixed

- **Smart Search is more resilient to slow Agents and brief status-read failures.** Planning gets a longer quiet window, transient read-only polling failures retry in place, and failed runs report their current stage promptly instead of ending as a generic timeout. Claude Agent 1.0.26, Codex Agent 1.0.12 and DeepSeek Agent 1.1.12 include the matching task updates; Codex also uses the current CLI feature-disable syntax and preserves early stderr diagnostics.

## v6.903.4 — 2026-09-03

### Changed

- **Smart Search now understands a question before it searches.** The selected Agent turns natural-language intent into a strictly validated search plan; dates such as “last month” become timezone-aware document constraints instead of keywords, while paths, tags, types, origins and sort choices remain locked by the host. Low recall may trigger one bounded tuning pass, then the host re-reads and freezes the cited Vault lines before an input-only Answer run. Typing still shows a zero-token Quick Preview, clearly separated from authoritative Intelligent Results.

- **Claude Agent 1.0.25, Codex Agent 1.0.11 and DeepSeek Agent 1.1.11 add phase-aware model routing.** Plan and Tune default to each harness's fast profile while Answer keeps its own default/quality choice; the Smart Search model menu stores these choices per provider and phase. Every run reports the actual resolved model, and all three providers now expose the shared input-only `search-plan` task plus capability negotiation for safe orchestration.

- **Memory 2.4.2 can copy a rigorous Role/Scope initialization prompt for external Agents.** The staged workflow derives candidates only from attributable Vault evidence, keeps clients and family in separate security domains, validates a complete Registry candidate, then pauses for owner confirmation before it prepares exact Claim reassignment proposals. Unsupported Registry writes, direct apply, forged human identity, credentials and ambiguous cross-Realm classification are explicitly forbidden.

- **Memory 2.4.1 brings Role and Scope governance into the main workspace.** “Identity & Context” is now a first-class tab beside confirmed memories, pending suggestions and history instead of a separate sheet. Active Role/Scope counts stay visible, and Registry edits plus atomic batch reassignment remain in place without interrupting the rest of the review flow.

## v6.903.3 — 2026-09-03

### Added

- **Memory 2.4.0 keeps one person's roles and working contexts genuinely isolated.** Create, rename, archive and restore Roles and Scopes in one Memory window, preview changes, then reassign existing memories in bulk. Agents get read-only and proposal-oriented CLI commands for batch work but cannot bypass human approval. One `MEMORY.md` remains the only projection, grouped Scope → Role with explicit Agent guidance; the legacy `USER.md` projection is removed on rebuild. Bulk apply binds the current Registry, claim heads and preview hash, so an interrupted or stale plan cannot become partially active.

## v6.903.2 — 2026-09-03

### Added

- **Smart Search & Answers turns the vault into a keyboard-first research surface.** Open it with Command/Ctrl-K, or Option-Space anywhere on macOS; results appear while you type, stay grouped by their real source, and can be opened in the editor or removed singly or in a standard multi-selection without touching the original files. Return asks the selected Claude, Codex or DeepSeek Agent using a frozen evidence packet plus policy-approved USER and MEMORY facts. Answers keep clickable source citations; a helpful vote archives an auditable Markdown answer under `answers/`, and a separate action generates a longer editable note.

- **Meetings 1.0.0 safely brings Hemory conversations into `ssot/meetings/`.** Its window and `meetings-import-hemory` CLI share the same dry-run-first migration engine, preserve the original time-coded speaker transcript and optional summary, write only the agreed flat metadata, never copy audio, and protect local edits, duplicate imports and interrupted transactions with hashes, checkpoints and recovery journals.

- **What you wrote by hand now says so, in the file.** New documents, quick notes, companion `.note.md` files, daily notes, pages created from a `[[wikilink]]`, Idea Spark originals and Trace Source requests — anything produced by a direct action of yours — now carry a signature in their metadata (`generated: by: human:<your git identity>`). Until now "a person wrote this" could only be inferred from the *absence* of an AI stamp, which made an unstamped AI dump indistinguishable from your own draft. Three states, each saying its own name: written by a person, written by a generator, claimed by nobody. The signature is written once, at creation — reopening and saving an old note will not grow one — and imported content (Roam, ebooks) stays unsigned, because carrying something across is not writing it. It reads the same in Obsidian, or in grep.

### Changed

- **Claude Agent 1.0.24, Codex Agent 1.0.10 and DeepSeek Agent 1.1.10 add a shared input-only `search-answer` task.** The task receives only the host's frozen question, approved memory facts and search snippets. It runs outside the vault, does not inherit tab or source context, rejects tool escalation, and keeps short answers and detailed Markdown on the same citation and evidence contract.

- **Claude Agent 1.0.23, Codex Agent 1.0.9 and DeepSeek Agent 1.1.9 reveal recent runs five at a time.** Each window starts with the five newest runs and a compact “Show 5 more” action; every click reveals the next five, with the final action matching the actual remainder. Changing task or history scope returns to the newest five, while background refreshes keep the amount you already expanded.

- **Claude Agent 1.0.22, Codex Agent 1.0.8 and DeepSeek Agent 1.1.8 put recent runs first and keep task templates compact.** Tasks are grouped by the feature plugin that uses them and every group starts collapsed, leaving the sidebar focused on the newest run history. Expand a neutral plugin row to choose a task; its count and running state stay visible while collapsed. Existing vaults are recognized without migration, and custom tasks remain available in their own group.

### Fixed

- **Command/Ctrl-A once again selects the whole document in every main-editor mode.** Source mode no longer loses the shortcut to the native menu responder, and the shared selection handling remains consistent across rich and source editing.

- **Clicking a `[[wikilink]]` that doesn't exist yet, in rich text, no longer creates an empty shell.** It used to write a 0-byte file: no title, no metadata, failing the format check and ranked last in search. It now takes the same page-creation path as the outline panel, and what lands is a well-formed document.

## v6.903.1 — 2026-09-03

### Added

- **Pending Memory suggestions now support multi-selection and safe bulk cleanup.** Click normally to select one, use Command/Ctrl to toggle items, or Shift to select a range; right-clicking the selection opens bulk ignore, reject and delete-candidate actions with an exact impact confirmation. Human approval, editing and importance remain deliberately item-by-item so distinct identity, fact, behavior and lifecycle meanings cannot be approved under one vague gesture.

### Changed

- **Claude Agent 1.0.21, Codex Agent 1.0.7 and DeepSeek Agent 1.1.7 share a clearer workspace design.** The environment, tasks and run history now live in one accessible sidebar scroll area while Settings stays pinned at the bottom, so longer task lists can no longer squeeze history out of view. Two-line history cards keep status, task, time and provider readable, and the input, stream, log and settings surfaces now match the Plugin Market's neutral visual language with consistent hover, selection and keyboard focus feedback.

- **The `notemd` CLI now has one complete, strict contract across built-in and plugin commands.** Top-level and topic help document the real arguments, value placeholders, JSON envelopes, quiet behavior and exit codes; Memory also exposes exact claim lookup, filtered lists, context manifests, purge plans and drift-aware checks as documented.

### Fixed

- **A concurrent change no longer makes a pending bulk action look all-or-nothing.** Items are guarded and processed serially; if one becomes stale, Memory stops, refreshes the real queue and reports exactly how many were completed and how many remain.

- **Plugin and automation commands no longer silently reinterpret bad input.** Dynamic plugin arguments keep their declared names and types, disabled plugins can be inspected and re-enabled, and unknown or conflicting flags fail safely. Share record cleanup works after the source file is gone, Reading Insights validates date ranges consistently, CLI-only runs no longer activate unrelated startup plugins, and WebView commands time out instead of hanging forever.

## v6.902.9 — 2026-09-02

### Added

- **Memory can now pull in what other AI assistants remember about you.** "复制导入记忆Prompt" in the Memory window copies a ready-made prompt; paste it into ChatGPT, Gemini or any other assistant with a memory feature and it exports its own entries as `notemd memory propose` commands. Running them in your vault files everything as pending claims — nothing enters memory until you confirm it here.

- **Confirmed Memory claims are now grouped by projection and category.** `MEMORY.md`, `USER.md` and structured-only context have distinct sections, with category counts inside each. The default order keeps important claims first and otherwise mirrors projection order; recent, oldest and text ordering are also available.

### Changed

- **Decision 1.1.6 uses its shorter product name everywhere.** Plugin menus, the window, the board, the marketplace and all four localized names now consistently show Decision / 决策 / 意思決定 / Entscheidung; the plugin ID and stored data remain unchanged.

### Changed

- **Decision 1.1.6 uses its shorter product name everywhere.** Plugin menus, the window, the board, the marketplace and all four localized names now consistently show Decision / 决策 / 意思決定 / Entscheidung; the plugin ID and stored data remain unchanged.

### Fixed

- **A proposed Claim can now allow several retrieval purposes.** `notemd memory propose` takes a comma-separated `--purpose`, matching the Memory window's own multi-select; previously a CLI-proposed Claim was limited to one purpose and was withheld from every other context. `replace` and `revoke` no longer demand `--category`, `--space` and `--purpose` that they go on to ignore.

## v6.902.8 — 2026-09-02

### Added

- **Memory 2.2.1 lets people correct confirmed and pending memory text before relying on it.** Editing a confirmed claim creates an auditable replacement instead of rewriting history; editing a pending suggestion approves the corrected text in one action while retaining the original proposal. Exact revision heads prevent concurrent changes from being overwritten, and lifecycle proposals remain protected from text edits.

## v6.902.7 — 2026-09-02

### Fixed

- **The first valid Memory proposal now initializes Memory Protocol v2 automatically.** External Agent harnesses can write a pending `propose create` into a completely empty Vault without a separate trusted-UI setup step; read-only or invalid commands, incomplete repositories, restricted content and non-empty existing projections remain fail-closed, while concurrent first writers share one idempotent protocol initialization.

## v6.902.6 — 2026-09-02

### Added

- **Claude Agent 1.0.20, Codex Agent 1.0.6 and DeepSeek Agent 1.1.6 report token usage and cost after every completed run.** Each plugin can show the summary in the completion tip or result area and keeps it in run history. Provider-reported charges take precedence; otherwise known models use dated official API list-price estimates that are visibly marked as estimates rather than subscription usage or an actual bill. Claude distinguishes prompt-cache write durations, Codex avoids guessing per-request long-context surcharges from aggregate turns, and DeepSeek selects its UTC peak or off-peak rate from the completion time. Harnesses that expose no usage continue to say so instead of deriving tokens from text length.

## v6.902.5 — 2026-09-02

### Added

- **Memory 2.2 can clear and rebuild all current memory from one guarded action.** Before anything changes, the plugin shows the exact number of confirmed claims and pending suggestions that will leave `USER.md`, `MEMORY.md` and Agent context. One confirmation removes those current views, safely resolves conflicting heads, resets inference to a full Vault scan, and keeps the owner identity, Memory protocol and immutable audit history.

### Changed

- **Memory inference now prefers no suggestion over a weak suggestion.** A six-part quality gate accepts only durable, useful statements explicitly made by the Vault owner; tasks, implementation details, product defaults, document summaries and one-off behavior inferences are excluded. Confirmed, pending, rejected and ignored history all participate in semantic deduplication, and a normal run usually proposes zero to five items instead of treating the cap as a target.

### Fixed

- **Trace Source 1.2.3 opens Inbox items inside its own window for editing.** Finished reports and saved requests now load into the plugin's existing editor instead of raising the main editor; edits autosave to the original file, and switching, slow writes, deletion and window closing are ordered so current text is not lost or recreated after deletion.

## v6.902.4 — 2026-09-02

### Changed

- **Ebook Import 1.3.5 expands managed topic taxonomies from five to eight topics.** AI topic design now chooses 2–8 natural domains without filling the limit or merging unrelated fields merely to stay under it; manual topic management and batch classification accept the same expanded catalog.

### Fixed

- **Plugin Editor Kit windows now select the whole document with Cmd+A or Ctrl+A in both rich and source modes.** Rich editing intercepts the shortcut before a code block can narrow the selection, while source editing explicitly selects its textarea instead of depending on the embedded WebView's native responder.

## v6.902.3 — 2026-09-02

### Added

- **Ebook Import 1.3.4 can classify every previously unclassified book as one reviewable AI batch.** The Agent reads bounded evidence from the newest AI digest, chapter headings, opening text and bibliographic metadata, suggests only existing topics, and leaves every assignment editable until one transactional confirmation updates book metadata and rebuilds all generated topic indexes.

### Fixed

- **Memory 2.1.1 can persist AI-inferred proposals under the Codex workspace sandbox.** Its repository lock no longer writes into protected Git metadata, linked worktrees still share one lock, and a run is reported as successful only after the matching inference checkpoint is present. Commits created while a scan is running are safely deferred to the next incremental pass instead of invalidating the current run.
- **Claude Agent 1.0.19, Codex Agent 1.0.5 and DeepSeek Agent 1.1.5 preserve complete terminal responses for application workflows.** Human-facing run history remains capped at an 8 KiB summary, while a separate bounded result channel prevents long YAML proposals from being parsed from a truncated tail; Codex also combines multi-message final responses, and Ebook Import retries one malformed protocol response before reporting failure.

## v6.902.2 — 2026-09-02

### Changed

- **The Plugin Market now shows the running note.md version and restart guidance below its subtitle.** After installing or updating a plugin, users are reminded to quit and reopen note.md so startup-loaded integrations take effect consistently.

### Fixed

- **Ebook Import 1.3.3 restores the Agent picker for AI topic design.** Topic design now remembers its own Claude, Codex, or DeepSeek choice independently from AI book reading, while the UI states whether the selected Agent is restricted to the inventory file or may read the Vault under a read-only task policy.
- **Plugin Market refresh now retrieves the current online catalog instead of accepting a cached index.** A user-initiated refresh carries a unique request URL and explicit no-cache policy, while startup still presents installed plugins from local cache first.
- **The native Plugins menu now matches the Plugin Market's current category layout.** AI appears first; Memory and the Claude, Codex, and DeepSeek Agents are grouped under AI, while Next and OpenClaw Chat remain under Move Forward.

## v6.902.1 — 2026-09-02

### Added

- **Memory 2.1 can infer durable owner memory from the existing Vault with a selected AI Agent.** The first successful run performs a full scan; later runs use a successful Git checkpoint for incremental inference. Every result remains a pending Claim for human review, and prompt-injection, secret, third-party and revocation boundaries are embedded in the task protocol.

### Changed

- **Memory now has one pure v2 path.** The Host, CLI and plugin no longer expose v1 compatibility, migration RPCs or legacy projection markers; an empty Vault is initialized directly as Memory Protocol v2 by the trusted UI action.

### Fixed

- **macOS installers now carry notarization on the disk image itself.** Both Apple Silicon and Intel DMGs are submitted to Apple, stapled and checked by Gatekeeper before publication, instead of relying only on the notarized app inside.

## v6.901.10 — 2026-09-02

### Fixed

- **Ebook Import 1.3.2 no longer lets one legacy directory stop AI topic design.** Unsafe book directory paths, such as names ending in whitespace, are left out of the AI inventory while valid books continue through classification; the inventory protocol itself remains strict against handcrafted unsafe paths.
- **Memory 2.0.1 now fails closed across approval, authority and concurrent Git paths.** Control-plane revisions must prove their causal authority; conflicting approve/reject decisions stay explicit; request idempotency is enforced within one clone and deterministically reconciled across clones; Context Manifests bind the exact preview; generated `USER.md` and `MEMORY.md` are recognized as read-only projections and retain a conservative warning during action-sensitive conflicts.
- **Ebook Import 1.3.1 makes topic organization transactional and capability-bounded.** Topic catalogs use revision checks and rollback-safe index rebuilds, symlink escapes are rejected, interrupted work validates the current library, concurrent AI status events are retained, and topic design accepts only the provider whose single-file read allowlist can be verified before untrusted metadata is processed.
- **Plugin updates no longer combine an old window with a new backend.** Replaced runtimes are permanently fenced, superseded plugin windows are destroyed before the installed package changes, and a same-ID process is reused only when its complete manifest and install directory still match.

## v6.901.9 — 2026-09-01

### Added

- **Memory 2.0 introduces a Git-backed personal Claim ledger.** Immutable YAML revisions now distinguish preferences, boundaries, decisions, beliefs, observations and material facts by subject, assertion, approval meaning, valid time, trust, risk and context scope. Agents may only propose pending claims about the Vault owner; a human decision is bound to the exact protocol, authority and revision.
- **Ebook Import now organizes every new book into a managed topic.** A library can define up to five topics, choose one during import, maintain generated topic indexes, and ask the configured AI provider to design a topic system from the existing collection without handing it uncontrolled write access.

### Changed

- **`USER.md` and `MEMORY.md` are now disposable plain-text projections.** The Host deterministically rebuilds their one-level categories and multiline facts from `.notemd/memory`, keeps process metadata out of the Markdown, and fails closed on damaged authority or source conflicts.
- **Memory remains conservative across devices and external models.** Independent Git clones merge immutable records as a set, concurrent edits to one Claim require explicit resolution, action-sensitive conflicts tighten permissions, and Context Manifests record the Space, purpose, caller, provider, model, tools and selection reasons used for Agent context.
- **AI is now the first system category in the Plugin Market.** Claude Agent, Codex Agent, DeepSeek Agent and Memory are grouped together at the top, including when older installed metadata or cached catalog entries still use their previous categories.

## v6.901.8 — 2026-09-01

### Changed

- **The Plugin Market now uses a standard, quieter window palette.** Window, section, card, update and AI surfaces use neutral system colors; category and plugin icons retain their visual identity, while actions share the system accent color.
- **Manual Memory confirmation is now a single action.** Clicking Confirm on a fact, or Approve on a reviewed proposal, immediately writes the SHA-bound human decision without asking for a second confirmation.
- **Memory projections no longer expose citation notes after every fact.** Inline footnote markers, footnote definitions and per-entry source properties stay out of `USER.md` and `MEMORY.md`; immutable proposals retain provenance and decisions bind the exact candidate SHA.

## v6.901.7 — 2026-09-01

### Added

- **Memory facts now have one-click review actions.** Confirm, deny, mark important, mark ignorable, or delete a fact without filling out the full metadata form. Pending facts reuse their exact original candidate, and every write still requires the in-window SHA-bound confirmation step.

### Changed

- **Deleting a memory now has explicit, auditable semantics.** An approved delete removes only the current `USER.md` or `MEMORY.md` projection block while retaining the immutable candidate and decision event; hand-written update candidates without an exact target revision fail closed.
- **Next card titles are now the direct editing entry point.** Clicking an Idea or Task title opens its content and metadata editor without adding another competing card control.

## v6.901.6 — 2026-09-01

### Added

- **Memory entries now separate review status from meaning and evidence.** Each entry can carry critical/high/normal/low priority, positive/negative/neutral behavior direction, epistemic status, certainty, an explicit Agent usage rule and a “must avoid” rule. Existing entries migrate conservatively as neutral and unknown instead of being promoted to confirmed facts.

### Changed

- **Memory now uses a focused macOS-style master-detail review window.** Pending, negative and critical material is easier to distinguish, typography and controls are consistent, and the in-window confirmation sheet shows the exact candidate identity, SHA-256, content and behavior metadata before a decision is written.
- **Idea and Task card metadata is fully editable from Next.** Clicking a project, priority, due-date or context chip now opens the complete metadata editor and keeps the Markdown frontmatter and lifecycle ledger aligned.

### Fixed

- **Plugin windows accept the first click after activation.** Memory no longer depends on a browser confirmation dialog, and interactive controls respond immediately when their window is brought forward.
- **Invalid Memory updates fail closed.** Update proposals require a non-empty target, owner identity uses its dedicated create/replace flow, and owner records are no longer exposed through ordinary memory editing controls.

## v6.901.5 — 2026-09-01

### Added

- **Next now manages Tasks as well as Ideas.** Each Task is stored as its own Markdown file under `inbox/tasks`, can be created directly as the current action, and keeps its content separate from the human-controlled lifecycle ledger. Vault instructions teach agents to add only concrete work owned by the configured Vault owner, with project, source and deduplication metadata instead of silently committing or completing work on the user's behalf.
- **Ideas and Tasks now carry practical action context.** Priority, due date and GTD context are visible and editable on every card; defaults live in Next's own settings page. The In Progress lane shows a persistent, configurable WIP warning based on the complete lane rather than the current search or project filter.
- **Vaults can share a durable user model and long-term memory with agents.** New Vault templates include `USER.md` for owner identity and stable profile facts, plus `MEMORY.md` for sourced cross-session facts and decisions. The new Memory plugin lists current entries, reviews exact before/after proposals and surfaces deterministic cleanup suggestions.
- **Agents can propose controlled memory changes from the command line.** `notemd memory` supports inspection, suggestions, create/replace/merge/revoke/priority proposals, integrity checks and migration. Approval or rejection is bound to the exact proposal SHA-256 and an explicit human actor; a proposal alone never becomes confirmed memory.

### Changed

- **Next settings now stay inside the Next window.** WIP limits and new-card defaults no longer add a private Next tab to note.md's system settings, while existing saved values continue to work.
- **Managed `USER.md` and `MEMORY.md` files are read-only projections.** Source and rich editors, manual and automatic saves, paste/drop, overwrite and history restore all refuse direct changes. Approved revisions are written atomically with immutable candidate and decision records; drift, stale revisions, changed proposal bytes and duplicate decisions fail closed.

## v6.901.4 — 2026-09-01

### Changed

- **The Plugin Market now separates cognitive work from system capabilities.** Capture, Read, Ideas, Move Forward, Reflect and Create describe what people accomplish with plugins; Import & Export and Experience are marked as system features. Roam Import and PDF Export live under Import & Export, while Power Mode has its own Experience category. The same order and wording now appear in the app, the native Plugins menu and the public marketplace.
- **Update every changed plugin in one action.** When updates are available, the Plugin Market presents a prominent Update All button. One confirmation authorizes the batch, each package is still verified before installation, and failures no longer prevent the remaining updates from completing.
- **AI-assisted plugins are easier to discover without turning AI into a catch-all category.** A compact “Create with AI” shelf links to focused collaborators for reading, inspiration, reasoning and action, and each matching card carries a specific AI role badge. Update alerts remain visually dominant.

### Fixed

- **Installed plugins move to the new categories immediately, including offline.** Official plugin IDs migrate old manifests and cached category names locally, while the refreshed marketplace index supplies the same taxonomy after the network connects.

## v6.901.3 — 2026-09-01

### Fixed

- **Sidecar answers now stay paired with the correct annotation when the same question appears more than once.** Each repeated question keeps its own answer card in the Markdown document—even when both annotations share a paragraph or an earlier duplicate is still unanswered. Adopting one answer and regenerating the sidecar also update only the matching occurrence instead of crossing answers between identical question texts.

## v6.901.2 — 2026-09-01

### Fixed

- **Running an Agent from Sidecar Notes now saves the note first.** Pending outline edits are flushed immediately, including a sidecar that has not been created on disk yet, and the Agent starts only after the actual saved path is confirmed. Save conflicts or failures stop the run instead of producing a misleading “outside the vault” error.

## v6.901.1 — 2026-09-01

### Added

- **Plugin tray shortcuts now become available immediately after installation.** Enabling, disabling or removing a plugin updates the registered system-wide shortcuts at the same time as the tray menu, without requiring note.md to restart.

### Changed

- **Localized plugin tray items keep a stable workflow order.** The visible names still follow the current interface language, while their position no longer jumps when the language changes. This lets Next remain directly below Idea Spark in every supported language.

## v6.831.2 — 2026-08-31

### Changed

- **The Plugin Market fits substantially more plugins on screen.** The header, category sections, grid gaps and cards are tighter while preserving clear category boundaries; plugins with an available update now receive an orange card highlight, target-version badge and matching primary action.

### Fixed

- **Plugin names and descriptions now follow the current interface language throughout the market.** Non-Western localized product names retain the English name in parentheses, while Western-language names stay concise. Installed plugins keep this metadata in the local-first cache, including for offline launches after the first refresh.

## v6.831.1 — 2026-08-31

### Changed

- **The Plugin Market is now organized as a spacious catalog instead of two dense lists.** Each capability category has one distinct section containing both installed and available plugins, with responsive cards that make descriptions, versions, enabled state, available updates and actions easy to scan.
- **Installed plugins appear before the network catalog finishes loading.** The market restores a small local snapshot first, reconciles it with the plugins on this device, then adds the full catalog and update state when the registry responds. Going offline no longer makes installed plugins disappear.

## v6.829.2 — 2026-08-29

### Changed

- **Copy context is now limited to the two files an agent needs.** The copied text gives only the source document's role and full path plus the sidecar note's role and full path, without an extra preamble or instructions.

## v6.829.1 — 2026-08-29

### Added

- **Sidecar Notes can copy a ready-to-use context prompt for another agent harness.** The Agent area now provides a Copy context button containing the open document's full path, the resolved sidecar-note path for its highlights, and a clear instruction to load both files. Successful copies confirm briefly in place; the button and copied prompt follow the current UI language.

### Fixed

- **Location Log can request macOS location access and explains permission failures clearly.** Signed builds now carry the Location entitlement required for the system authorization prompt. If access is disabled, the warning names the exact System Settings path instead of exposing only a `kCLErrorDomain code=1` error.

## v6.828.6 — 2026-08-28

### Added

- **More Mermaid diagram families and styling controls are available.** The bundled engine now includes Event Modeling, `cynefin-beta`, swimlanes and the railroad beta variants, plus donut pies, richer XY chart labels/legends, collapsible flowchart subgraphs, additional flowchart shapes and ER subgraphs.

### Changed

- **Mermaid has been upgraded from 11.14.0 to 11.17.2, and every note.md rendering path now uses that exact build.** Rich editing, preview, sharing, printing and PDF export no longer risk resolving different Mermaid versions. The engine remains lazy-loaded, so the extra diagram code does not enlarge note.md's initial application chunk.
- **New Vaults teach AI agents exactly which Mermaid they can generate.** The default `AGENTS.md` now names the bundled 11.17.2 grammar families, Unicode and styling support, beta boundaries, and reliable-generation rules. Existing Vault instructions remain untouched.
- **Several upstream renderer defaults intentionally produce cleaner output.** Class diagrams use Mermaid's unified renderer by default; C4 diagrams use unified pure-SVG shapes and wrap long labels; dotted class namespaces are hierarchical; and tree-view icons are hidden unless requested. Existing diagrams remain valid, but these diagram families may have small spacing, shape or marker differences from 11.14.
- **Mermaid custom CSS is parsed more strictly.** Invalid or unbalanced `classDef`/theme rules and unsafe at-rules are discarded instead of leaking into the page. State-diagram comments must use `%%`; a single `%` is now ordinary diagram text.

### Fixed

- **`quadrantChart` accepts unquoted Chinese, Japanese, Korean, accented and emoji labels.** Axis names, quadrant captions and data-point names no longer fail at the 11.14 lexer. `architecture-beta` titles and labels likewise accept unquoted Unicode and punctuation such as `/` and `-`.
- **Related Mermaid line, marker and label regressions are fixed together.** This includes restored flowchart/block `edgePaths`, single self-loop paths in flowcharts and state diagrams, correctly scaled class-relation markers, ELK arrowheads in dark themes, classDef text colors, wide block labels, C4 long-label wrapping, sequence `alt`/`else` and `rect` backgrounds, RTL message alignment, Gantt marker placement, radar labels, Venn unions, XY chart labels and large tree-view labels.
- **Mermaid 11.17 remains usable on note.md's older supported macOS WebViews.** A compatibility path supplies the CSSOM object needed by Mermaid's safer style sanitizer when WebKit exposes stylesheets but cannot construct `CSSStyleSheet` directly.

### Security

- **Mermaid and its SVG/CSS sanitizer dependencies include the current upstream security fixes.** The upgrade removes the Mermaid, DOMPurify and UUID advisories associated with the previous locked graph stack, including malformed diagram/CSS injection and denial-of-service cases.

## v6.828.5 — 2026-08-28

### Fixed

- **Rich editors in Idea Spark and Trace Source now follow theme fonts immediately.** Open plugin windows no longer race the settings save and briefly keep the previous theme, while Effie's bundled LXGW webfont is allowed through the plugin sandbox's narrowly scoped font policy instead of silently falling back to a system font. The fix comes with the note.md app; no separate plugin update is required.

- **Idea Spark recognizes the existing `*-idea.md` filename convention again.** The inbox now lists only ordinary files with that exact lowercase suffix; new and renamed ideas use the same suffix, while proof documents, other Markdown files, directories and `.idea.md` names stay out of the list.

## v6.828.4 — 2026-08-28

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
