// store.svelte.ts — the Idea Spark window's single state container.
//
// Split in two on purpose (same shape as plugins-src/decision-log):
//
//   * **Pure transitions** (`applyRunDone`, `markPending`, `setIdeaDir`,
//     `statusOf`, …) take the store as their first argument. They never touch
//     the bridge and never read the module singleton, so `store.test.ts` can
//     build an isolated store with `createStore()` and assert on transitions
//     without a component tree, a host, or a DOM.
//   * **Actions** (`boot`, `reload`, `saveIdea`, …) are the IO half: they call
//     the host bridge and then apply pure transitions to the singleton `state`.
//
// Reactivity discipline: nothing here is called from a `$effect`. App.svelte
// drives the whole lifecycle from `onMount` and from event handlers, because a
// `$effect` that synchronously calls a function which reads *and* writes
// `$state` self-invalidates into a loop that freezes the window (v4.2.4).
import { agentStatus, bridge, vaultExists, vaultInfo, vaultList, vaultRead, vaultRemove, vaultRename, vaultWrite } from './bridge'
import { interpretStatus, TASK_ID, type RunView } from './agent-client'
import { buildIdeaDoc, rebuildIdeaDoc } from './idea-doc'
import { proofPathFor, splitFrontmatter, timestampFileName, titleFromMarkdown } from './naming'
import { isReservedConceptName } from './okf/concept'
import { DEFAULT_STATE, parseState, serializeState, STATE_PATH } from './state-io'
import { deriveStatus, listIdeas, type IdeaStatus } from './status'
import { t, type MessageKey } from './strings'

export interface SparkStore {
  /** Absolute vault root, or null when no vault is open (→ `needVault`). */
  vaultRoot: string | null
  /** True once the startup sequence found no open vault; the UI shows only a hint. */
  needVault: boolean
  /** True until the startup sequence settles (vault info + state + listing). */
  booting: boolean
  /** Set when the Editor Kit failed to load; the UI falls back to a textarea. */
  kitFailed: boolean
  /** Set when the last directory listing failed outright (as opposed to being
   *  empty). Purely a UI signal: it lets the history list say "couldn't read"
   *  instead of implying "you have no ideas yet". The save path doesn't consult
   *  it — `freeFileName` asks the disk unconditionally, which covers a stale
   *  listing just as well as a failed one. */
  listFailed: boolean

  /** Vault-relative directory ideas live in. */
  ideaDir: string
  /** Every file in `ideaDir` as a vault-relative path — proof sidecars included,
   *  because `statusOf` derives "done" from a sidecar's presence. */
  files: string[]
  /** Idea file names (bare, newest first) — what the history list renders. */
  docs: string[]
  /** ideaRelPath → run_id for runs the agent is currently working on. */
  pending: Record<string, string>
  /** ideaRelPaths whose most recent run ended in a non-success terminal state. */
  failed: string[]

  /** Bare file name of the idea in the editor; null while it has never been saved. */
  current: string | null
  /** Raw frontmatter of the idea on disk (fences excluded), null for a document
   *  that has never been saved. Carried across saves so re-saving preserves
   *  `created` and any key this plugin doesn't know about — the editor only
   *  holds the body, so without this they would be rewritten away. */
  currentFrontmatter: string | null
  /** Editor content as of the last successful save — the dirty-check baseline. */
  savedMarkdown: string
  /** True when the editor content differs from `savedMarkdown`. */
  dirty: boolean

  /** A host call is in flight (save/settings); disables the action bar. */
  busy: boolean
  /** Raised by a successful run; `<Celebration/>` consumes and clears it. */
  celebrate: boolean
  /** Bumped with every raise. `celebrate` alone can't distinguish "still the
   *  first burst" from "a second run just finished", so the animation timer
   *  would never restart — and the first burst's timer would cut the second
   *  one short. */
  celebrateSeq: number
  /** Proof document of the most recent successful run. Written by
   *  `applyRunDone`; the celebration's "open result" affordance that reads it
   *  lands with the delegation chain (Task 13) — until then the history list's
   *  per-row button is the only way in. */
  lastResult: string | null

  /** Whether the inbox panel is expanded. Persisted (`state-io.ts`). */
  inboxOpen: boolean
  /** Bare file name → the title read out of that idea's body — the inbox's row
   *  label cache. Populated lazily, one read per row that actually scrolls into
   *  view (`ensureTitle`), because a directory of a few hundred ideas would
   *  otherwise cost a few hundred bridge round trips just to draw a list.
   *  A name absent from here renders as `displayName(name)` (see `titleOf`).
   *  Kept honest by this window's own writes (`saveIdea`/`loadIdea`/`renameIdea`/
   *  `deleteIdea`) and dropped wholesale by `invalidateTitles` when the window
   *  regains focus — this is not the only process that edits these files. */
  titles: Record<string, string>
  /** Rotation counter for the blank-document placeholder line (`placeholder.ts`).
   *  Persisted so the line shown does not reset to the same one on every
   *  restart. */
  placeholderSeq: number
  /** Autosave's own status, distinct from `busy` (which the action bar reads
   *  to disable itself during an explicit host call). */
  saveState: { kind: 'idle' } | { kind: 'saving' } | { kind: 'saved'; at: string } | { kind: 'failed'; message: string }
}

/** A run's terminal outcome, as read back from `host.agent.status`. */
export interface RunDone {
  run_id: string
  /** claude-agent vocabulary: `success` | `error` | `timeout` | `cancelled` | `lost`. */
  status: string
  /** Proof document the run produced; derived from the idea path when absent. */
  open_path?: string
}

export function createStore(): SparkStore {
  return {
    vaultRoot: null,
    needVault: false,
    booting: true,
    kitFailed: false,
    listFailed: false,
    ideaDir: DEFAULT_STATE.ideaDir,
    files: [],
    docs: [],
    pending: {},
    failed: [],
    current: null,
    currentFrontmatter: null,
    savedMarkdown: '',
    dirty: false,
    busy: false,
    celebrate: false,
    celebrateSeq: 0,
    lastResult: null,
    inboxOpen: false,
    titles: {},
    placeholderSeq: 0,
    saveState: { kind: 'idle' },
  }
}

export const state: SparkStore = $state(createStore())

// ── pure transitions ────────────────────────────────────────────────────────

/** `ideaDir` + a bare file name → the vault-relative path used as a store key. */
export function relPath(s: SparkStore, name: string): string {
  return `${s.ideaDir}/${name}`
}

/** Status of one idea (by bare file name), derived from the current snapshot. */
export function statusOf(s: SparkStore, name: string): IdeaStatus {
  return deriveStatus(relPath(s, name), new Set(s.files), s.pending, new Set(s.failed))
}

/**
 * Whether ANY run is in flight — the single source of truth for "can another
 * idea be delegated right now".
 *
 * Per-TASK, not per-idea, and that is not a UI preference: claude-agent locks
 * a task's run directory for the duration of a run ("Same task mutually
 * exclusive", `lock.rs`; `engine::run` takes `lock::acquire(task_run_dir)` as
 * its first act, and `task_run_dir` is `runs_root/idea-proof` for every idea
 * we ever delegate). A second `run-task` while one is live still comes back
 * with a `run_id` — the refusal happens later, inside the spawned task — so
 * the second run would look started, write no record at all, and surface two
 * seconds later as `{state:'lost'}`: a ⚠ and a "the agent couldn't argue this"
 * toast about an idea that was never attempted, on top of claude-agent's own
 * failure reminder for it.
 *
 * Guarding on `pending` alone (rather than on `statusOf`) is the other half:
 * `deriveStatus` ranks `done` above `running`, so an idea that already has a
 * `.proof.md` reports `done` while its re-run is live — a menu item keyed on
 * that would be enabled while the action bar's own button was disabled.
 */
export function runInFlight(s: SparkStore): boolean {
  return Object.keys(s.pending).length > 0
}

/**
 * Registers a run against an idea: it becomes `running` and any earlier
 * failure is forgotten, so a retry doesn't render as failed while it runs.
 */
export function markPending(s: SparkStore, ideaRel: string, runId: string): void {
  s.pending = { ...s.pending, [ideaRel]: runId }
  s.failed = s.failed.filter((f) => f !== ideaRel)
}

/**
 * Applies a run's terminal outcome. `success` → the idea's proof document is
 * folded into the listing (so `statusOf` derives `done` without waiting for a
 * re-list) and `celebrate` is raised; every other status → `failed`.
 *
 * Returns the resulting status, or `null` when no pending run matches
 * `run_id` — a stale push from another session must not celebrate, fail, or
 * mutate anything.
 *
 * What goes into `files` is always the CONVENTIONAL proof path
 * (`proofPathFor(idea)`), never `ev.open_path` verbatim: `deriveStatus` and
 * `openResult` both key off that convention, so folding in an off-convention
 * path would return 'done' while the row still rendered as a draft with no way
 * to open anything. `open_path` is kept — verbatim — in `lastResult`, which is
 * the field that means "the artifact this run actually produced".
 */
export function applyRunDone(s: SparkStore, ev: RunDone): IdeaStatus | null {
  const ideaRel = Object.keys(s.pending).find((k) => s.pending[k] === ev.run_id)
  if (ideaRel === undefined) return null

  const { [ideaRel]: _done, ...rest } = s.pending
  s.pending = rest

  if (ev.status !== 'success') {
    if (!s.failed.includes(ideaRel)) s.failed = [...s.failed, ideaRel]
    return 'failed'
  }

  const proof = proofPathFor(ideaRel)
  if (!s.files.includes(proof)) s.files = [...s.files, proof]
  s.failed = s.failed.filter((f) => f !== ideaRel)
  s.lastResult = ev.open_path ?? proof
  s.celebrate = true
  s.celebrateSeq += 1
  return 'done'
}

/**
 * Canonical form of a user-typed idea directory, or null when it isn't a plain
 * vault-relative directory: empty/blank, absolute (`/…`), or containing `..`
 * anywhere. The `..` substring check is deliberately stricter than a segment
 * check — the host's own path guard rejects `..` outright and no legitimate
 * idea directory has a double dot in its name.
 *
 * Exported so the settings popover can grey out its save button live without
 * mutating anything.
 */
export function normalizeIdeaDir(dir: string): string | null {
  const trimmed = dir.trim()
  if (trimmed.startsWith('/')) return null
  if (trimmed.includes('..')) return null
  return trimmed.replace(/\/+$/, '') || null
}

/** Validates and applies a new idea directory; false leaves the store untouched. */
export function setIdeaDir(s: SparkStore, dir: string): boolean {
  const normalized = normalizeIdeaDir(dir)
  if (normalized === null) return false
  s.ideaDir = normalized
  return true
}

/**
 * `setIdeaDir` plus the bookkeeping a *real* directory change implies: the open
 * document lives in the old directory and stays there, so it is detached from
 * the editor. The next save then names a fresh, properly deduplicated file in
 * the new directory instead of silently cloning the old name into it.
 * Re-typing the same directory (or a differently-spaced spelling of it) is not
 * a change and leaves the open document attached.
 */
export function changeIdeaDir(s: SparkStore, dir: string): boolean {
  const before = s.ideaDir
  if (!setIdeaDir(s, dir)) return false
  if (s.ideaDir !== before) {
    s.current = null
    s.currentFrontmatter = null
  }
  return true
}

/** Bare names of every file the last listing saw (ideas + sidecars + anything else). */
export function fileNames(s: SparkStore): string[] {
  const prefix = `${s.ideaDir}/`
  return s.files.map((f) => (f.startsWith(prefix) ? f.slice(prefix.length) : f))
}

/**
 * The file name a save should write to: the one this idea already occupies, or
 * — for an idea that has never been saved — `YYYY-MM-DD-HHmm-idea.md` (the
 * creation moment, see `timestampFileName`) deduplicated against *every* file
 * in the directory (an orphaned `.proof.md` occupies a name just as much as
 * an idea does).
 *
 * Keeping the name once it exists is deliberate: renaming the document because
 * the user edited its title would scatter one idea across several files. The
 * markdown itself no longer factors into the name at all — see
 * `timestampFileName` for why.
 *
 * Note this only knows what the last listing saw. `saveIdea` re-checks the
 * winner against the disk before writing — see the note there.
 */
export function nextFileName(s: SparkStore, _markdown: string, nowIso: string): string {
  return s.current ?? timestampFileName(new Date(nowIso), new Set(fileNames(s)))
}

/**
 * The exact bytes a save writes. A never-saved idea gets fresh OKF frontmatter
 * stamped `nowIso`; an idea that came off disk keeps its own frontmatter (see
 * `rebuildIdeaDoc`: existing keys, `created` included, are preserved and only
 * missing ones are filled in), because the editor holds the body alone and
 * would otherwise rewrite that metadata away on every save.
 */
export function ideaDocText(s: SparkStore, markdown: string, nowIso: string): string {
  return s.currentFrontmatter === null
    ? buildIdeaDoc(markdown, nowIso)
    : rebuildIdeaDoc(s.currentFrontmatter, markdown, nowIso)
}

/**
 * The slice of the Editor Kit the dirty check needs. Declared structurally so
 * tests can stand in a stub — `KitEditor` itself lives behind a `plugin://`
 * dynamic import that no test can load.
 */
export interface EditorHandle {
  getMarkdown(): string
  setMarkdown(md: string): void
}

/**
 * Adopts whatever the editor currently holds as the dirty-check baseline.
 *
 * This MUST be the editor's own output rather than the text we handed it.
 * moraya's `setContent` dispatches a ProseMirror transaction, and the lazy
 * change plugin re-serializes the document ~200 ms later without distinguishing
 * a programmatic set from a keystroke — a round trip that normalizes the
 * markdown (a trailing newline, for one, does not survive it). Baselining on
 * the input would therefore mark an *untouched* document dirty a moment after
 * it loads, which in turn would make the save-before-switch write files nobody
 * asked for and re-serialize hand- or agent-written ideas into ProseMirror's
 * preferred spelling. Anchoring on `getMarkdown()` — the same
 * `serializeMarkdown(view.state.doc)` the plugin will later report — makes the
 * baseline byte-identical to the echo.
 */
export function rebaseline(s: SparkStore, editor: EditorHandle | null): void {
  if (editor) s.savedMarkdown = editor.getMarkdown()
  s.dirty = false
}

/** Pushes text into the editor, then re-baselines against what it actually holds. */
export function showInEditor(s: SparkStore, editor: EditorHandle | null, md: string): void {
  if (!editor) {
    // The fallback textarea is bound verbatim: what goes in is what it holds.
    s.savedMarkdown = md
    s.dirty = false
    return
  }
  editor.setMarkdown(md)
  rebaseline(s, editor)
}

/** Records the editor's reported content against the baseline. */
export function markEdited(s: SparkStore, md: string): void {
  s.dirty = md !== s.savedMarkdown
}

/**
 * Whether `liveMarkdown` must be persisted before the editor's content is
 * replaced. Callers pass the **live** buffer (`getMarkdown()`), never `dirty`:
 * the flag lags the editor by the change plugin's 200 ms debounce, so a user
 * who types a paragraph and immediately clicks a history row would otherwise
 * sail past an unset flag and lose exactly that paragraph.
 */
export function needsSaveBefore(s: SparkStore, liveMarkdown: string): boolean {
  return liveMarkdown !== s.savedMarkdown
}

/** A fresh idea starts blank: no pre-filled template, just a grey-text
 *  placeholder (`placeholder.ts`) prompting the user to write. */
export function ideaTemplate(): string {
  return ''
}

/**
 * Whether a document holds nothing but whitespace. Autosave writes on every
 * pause in typing, and a window is opened far more often than an idea is
 * actually written down — so a blank document is never given a file (see
 * `saveIdea`), or the inbox would fill up with empty rows nobody created on
 * purpose. Whitespace counts as blank: a stray newline from a mis-key is not
 * an idea either.
 *
 * The rule has a second half that is easy to miss: a blank buffer also never
 * **empties an idea that already has a file**. Selecting all and deleting does
 * not truncate the document on disk — the file keeps its last non-blank
 * content, `saveState` drops to `idle` (the bar must not go on claiming
 * "saved 19:42" about content the editor no longer shows), and closing the
 * window warns about unsaved changes like any other unwritten edit.
 *
 * That is deliberate: an idea is removed by deleting it (the inbox's job), not
 * by blanking the editor, and the same guard is the only thing standing
 * between a spurious empty `onChange` echo — the kit rebuilds its view on
 * `setMode` — and a saved idea being silently wiped. Trading "you cannot empty
 * a file from the editor" for "an editor hiccup cannot erase your idea" is the
 * right way round.
 */
export function isBlank(markdown: string): boolean {
  return markdown.trim() === ''
}

/**
 * Local `HH:mm` — the action bar's "saved 19:42". Local, not UTC, because it
 * answers "how long ago did that happen" for the person looking at the screen,
 * and stored pre-formatted in `saveState.at` so the bar renders a plain string
 * (nothing else ever needs the instant back).
 */
export function clockTime(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}`
}

/** On-disk idea text → what the editor shows: frontmatter (and the blank lines
 *  right after it) stripped, so the user edits their prose, not our metadata. */
export function bodyOf(md: string): string {
  return splitFrontmatter(md)[1].replace(/^\n+/, '')
}

/** `2026-08-04-my-idea.md` → `my-idea` — the history list's label. */
export function displayName(name: string): string {
  const base = name.endsWith('.md') ? name.slice(0, -3) : name
  const stripped = base.replace(/^\d{4}-\d{2}-\d{2}-/, '')
  return stripped || base
}

/**
 * The raw YAML of a leading frontmatter block (fences excluded), or null when
 * the document has none / never closes the one it opens. Shares `naming.ts`'s
 * single fence scanner with `bodyOf`, so the two halves can never disagree
 * about what counts as frontmatter.
 */
export function frontmatterOf(md: string): string | null {
  return splitFrontmatter(md)[0]
}

// ── inbox: row labels, deletion, renaming ───────────────────────────────────

/**
 * An idea's label in the inbox, derived from its own text: the body's H1, or
 * its first non-empty line, or — when there is no usable text at all — the file
 * name.
 *
 * The file name alone is not a usable label any more: names are creation
 * timestamps (`2026-08-04-1942-idea.md`), so a list of them says nothing about
 * what any of the ideas *are*. `md` is the whole document as read from disk;
 * frontmatter is skipped by `titleFromMarkdown` itself.
 *
 * `titleFromMarkdown`, not `slugFromMarkdown`: the latter builds *file names*
 * (spaces → hyphens, punctuation deleted, 40 code points hard cut) and a row
 * labelled `Ship-the-thing` is not showing the user their title. See the note
 * on `titleFromMarkdown` in naming.ts.
 */
export function rowTitle(md: string, name: string): string {
  return titleFromMarkdown(md) ?? displayName(name)
}

/** Cached row label for `name`, or the file-name fallback until its body has
 *  been read (`ensureTitle`). Pure — the read is the action's job. */
export function titleOf(s: SparkStore, name: string): string {
  return s.titles[name] ?? displayName(name)
}

/**
 * The creation instant encoded in an auto-generated file name —
 * `YYYY-MM-DD-HHmm-…` (the minute), or `YYYY-MM-DD-…` (local midnight) for the
 * older date-only spelling. Null when the name carries no date at all, which is
 * the normal state of a user-renamed idea, and null for an impossible date
 * (`2026-13-45`) rather than silently rolling it over into another month.
 *
 * The directory listing carries names only — no mtime — so the name is the one
 * timestamp the inbox has without reading every file's metadata.
 */
export function createdFromName(name: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:-(\d{2})(\d{2}))(?:\D|$)|^(\d{4})-(\d{2})-(\d{2})(?:\D|$)/.exec(name)
  if (!m) return null
  const [y, mo, d, h, mi] = m[1]
    ? [m[1], m[2], m[3], m[4], m[5]]
    : [m[6], m[7], m[8], '00', '00']
  const date = new Date(Number(y), Number(mo) - 1, Number(d), Number(h), Number(mi))
  // Round-trip check: `new Date(2026, 12, 45)` is a valid *Date*, just not the
  // date that was written down. Anything that doesn't come back identical was
  // never a real timestamp and must not be shown as one.
  const same =
    date.getFullYear() === Number(y) &&
    date.getMonth() === Number(mo) - 1 &&
    date.getDate() === Number(d) &&
    date.getHours() === Number(h) &&
    date.getMinutes() === Number(mi)
  return same ? date : null
}

/**
 * How long ago `from` was, as the `(value, unit)` pair `Intl.RelativeTimeFormat`
 * takes — negative values, i.e. in the past. Coarse on purpose: an inbox row has
 * room for "3 days ago", not for "3 days 4 hours".
 *
 * Formatting is the caller's (it needs the active locale); this half is pure so
 * the bucket boundaries can be pinned by tests. A `from` in the future — clock
 * skew, a file stamped by another machine — is clamped to "0 minutes ago"
 * rather than rendered as a promise about the future.
 */
export function relativeAge(
  from: Date,
  now: Date,
): { value: number; unit: 'minute' | 'hour' | 'day' | 'month' | 'year' } {
  // `-0` is not `0` to `Object.is` (and `Intl` has been known to render the two
  // differently), so the sign is only applied to a non-zero count.
  const past = (n: number) => (n === 0 ? 0 : -n)
  const minutes = Math.max(0, Math.floor((now.getTime() - from.getTime()) / 60_000))
  if (minutes < 60) return { value: past(minutes), unit: 'minute' }
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return { value: past(hours), unit: 'hour' }
  const days = Math.floor(hours / 24)
  if (days < 30) return { value: past(days), unit: 'day' }
  const months = Math.floor(days / 30)
  if (months < 12) return { value: past(months), unit: 'month' }
  return { value: past(Math.floor(days / 365)), unit: 'year' }
}

/**
 * Every file deleting `name` takes with it: the idea, plus its `.proof.md`
 * sidecar when the listing knows about one. Idea first, so a partial failure
 * leaves an orphaned proof (recoverable, and visible as such) rather than an
 * idea whose "done" badge points at a document that is no longer there.
 *
 * The idea itself is listed unconditionally — a row can outlive the listing it
 * was drawn from, and `host.vault.remove` is idempotent, so asking to delete a
 * file that has already gone is harmless.
 */
export function filesToDelete(s: SparkStore, name: string): string[] {
  const idea = relPath(s, name)
  const proof = proofPathFor(idea)
  return s.files.includes(proof) ? [idea, proof] : [idea]
}

/**
 * Validates a user-typed rename and resolves it to a file name.
 *
 * `.md` is appended when the user left it off (they are naming an idea, not a
 * file). Refused: blank, a path separator (this renames a file, it does not
 * move it out of the idea directory), a leading dot (hidden files, and `..`
 * with it), and — all four collapsed into `taken`, because from the user's side
 * that is exactly what they have in common, the name is not available:
 *
 *   1. a name another file in the directory already occupies;
 *   2. the OKF-reserved structural names `index.md` / `log.md` (see
 *      `okf/concept.ts`);
 *   3. anything ending in `.proof.md` — that suffix *means* "argument for the
 *      idea next door";
 *   4. a name whose own sidecar slot (`<name>.proof.md`) is already occupied.
 *
 * 2 and 3 are not pedantry: `listIdeas` filters BOTH out of the listing, so an
 * idea allowed to take such a name would vanish from the inbox on the spot —
 * still on disk, no error shown, no row left to undo it from, and (if it was
 * the open document) autosave still writing into it. 4 is the mirror image: an
 * orphaned `b.proof.md` would make the freshly renamed `b.md` claim a `done`
 * badge and an "open the argument" item pointing at a document that argues
 * something else entirely.
 *
 * Renaming a file to the name it already has is accepted, not refused as
 * `taken`: the caller then has nothing to do, and telling the user their own
 * name is unavailable would be nonsense.
 */
export function validateRename(
  s: SparkStore,
  from: string,
  raw: string,
): { ok: true; name: string } | { ok: false; reason: 'empty' | 'slash' | 'dot' | 'taken' } {
  const trimmed = raw.trim()
  if (!trimmed) return { ok: false, reason: 'empty' }
  if (trimmed.includes('/')) return { ok: false, reason: 'slash' }
  if (trimmed.startsWith('.')) return { ok: false, reason: 'dot' }

  const name = trimmed.endsWith('.md') ? trimmed : `${trimmed}.md`
  if (name === from) return { ok: true, name }
  if (isReservedConceptName(name) || name.endsWith('.proof.md')) return { ok: false, reason: 'taken' }
  const occupied = fileNames(s)
  if (occupied.includes(name) || occupied.includes(proofPathFor(name))) return { ok: false, reason: 'taken' }
  return { ok: true, name }
}

// ── actions (bridge IO on the singleton) ────────────────────────────────────

/** Fire-and-forget host toast; never rejects (a failed toast must not break a flow). */
export function toast(message: string, level: 'info' | 'error' = 'info'): void {
  void bridge()
    .request('host.toast', { level, message })
    .catch((e) => console.warn('[idea-spark] toast failed:', e))
}

/**
 * Startup sequence: vault info → `.notemd/idea-spark.json` → directory listing.
 * Every step degrades instead of throwing: no vault flips `needVault` and stops;
 * a missing/corrupt state file falls back to defaults; a missing idea directory
 * is an empty listing (it gets created on the first save).
 */
export async function boot(): Promise<void> {
  state.booting = true
  try {
    const info = await vaultInfo().catch(() => ({ root: null }) as { root: string | null })
    state.vaultRoot = info?.root ?? null
    if (!state.vaultRoot) {
      state.needVault = true
      return
    }
    const raw = await vaultRead(STATE_PATH)
      .then((r) => r.content)
      .catch(() => null)
    const persisted = parseState(raw)
    state.ideaDir = persisted.ideaDir
    state.pending = persisted.pendingRuns
    state.inboxOpen = persisted.inboxOpen
    state.placeholderSeq = persisted.placeholderSeq
    await reload()
  } finally {
    state.booting = false
  }
}

/**
 * Re-lists the idea directory.
 *
 * The host returns the same `io: …` error for "this directory doesn't exist
 * yet" (the normal first-run state) and for a real IO failure — `vault_list` in
 * `ui_rpc.rs` can't tell the caller which — so both land here as an empty
 * listing. What the two cases must NOT share is silence: `listFailed` records
 * that the emptiness is unverified so the history list can say so, rather than
 * letting an unreadable directory pass for an empty one.
 *
 * The save path does not read this flag. An empty-because-unreadable listing
 * would make `nextFileName` hand back an un-suffixed name and overwrite a
 * same-day, same-title idea, but the defence against that is `freeFileName`
 * asking `host.vault.exists` unconditionally — which also covers a listing that
 * is merely stale, a case no flag can detect.
 */
export async function reload(): Promise<void> {
  let failed = false
  const entries = await vaultList(state.ideaDir)
    .then((r) => r.entries ?? [])
    .catch((e) => {
      console.warn('[idea-spark] listing the idea directory failed:', e)
      failed = true
      return [] as { name: string; is_dir: boolean }[]
    })
  // Toast only on the transition into failure: `reload` runs after every save,
  // and a persistently unreadable directory would otherwise toast on each one.
  // The history list keeps showing the condition for as long as it lasts.
  const was = state.listFailed
  state.listFailed = failed
  if (failed && !was) toast(t('historyUnavailable'), 'error')
  state.files = entries.filter((e) => !e.is_dir).map((e) => relPath(state, e.name))
  state.docs = listIdeas(entries)
}

/**
 * Persists `.notemd/idea-spark.json`. Failure is reported, never thrown.
 *
 * Exported because `pending` is the one piece of this state that MUST reach
 * disk the moment it changes: a run registered only in memory is lost to a
 * window close (or a crash), and nothing would ever reconcile it — the idea
 * would sit as a draft while claude-agent quietly finished arguing it.
 */
export async function persist(): Promise<void> {
  try {
    await vaultWrite(
      STATE_PATH,
      serializeState({
        ideaDir: state.ideaDir,
        pendingRuns: { ...state.pending },
        inboxOpen: state.inboxOpen,
        placeholderSeq: state.placeholderSeq,
      }),
    )
  } catch (e) {
    console.error('[idea-spark] writing plugin state failed:', e)
  }
}

/** `RunView`'s terminal kinds → the claude-agent status word `applyRunDone`
 *  speaks. Only `success` is a success; everything else is a failed run. */
export function runStatusWord(view: { kind: 'done'; success: boolean } | { kind: 'lost' }): string {
  if (view.kind === 'lost') return 'lost'
  return view.success ? 'success' : 'error'
}

/**
 * Applies a terminal outcome and lands the consequences: the pending entry is
 * dropped from the state file (so a restart doesn't ask about a run that has
 * already ended) and the directory is re-listed (so the new `.proof.md` is
 * really there rather than just folded in optimistically by `applyRunDone`).
 *
 * Returns what `applyRunDone` did, `null` included — a `run_id` matching no
 * pending run is a stale answer and must change nothing at all, disk writes
 * least of all.
 */
export async function finishRun(runId: string, status: string): Promise<IdeaStatus | null> {
  const outcome = applyRunDone(state, { run_id: runId, status })
  if (outcome === null) return null
  await persist()
  await reload()
  return outcome
}

/**
 * One-shot correction of `pending` at startup, and the answer to "what should
 * this window keep watching".
 *
 * `pending` comes off disk, so every entry in it was written by a *previous*
 * window. In the meantime the run may well have finished (claude-agent is
 * resident and outlives this window by design — that is why it, and not we,
 * sends the tray reminder) or died with the machine. Asking once at startup is
 * what keeps a ⏳ from becoming permanent.
 *
 * Three answers, three treatments:
 *   * `done`   → folded in, dropped from disk (celebration suppressed, see below);
 *   * `lost`   → marked failed, dropped from disk;
 *   * `running`→ left exactly as it is, and handed back so the caller can
 *                resume the inline progress poll for it.
 *
 * A status call that *rejects* (claude-agent uninstalled or disabled since the
 * run was started) also leaves the entry alone: "I can't reach the agent" is
 * not evidence that the run failed, and marking it failed would be a lie that
 * outlives the outage.
 *
 * The celebration is deliberately swallowed here. Confetti belongs to a run
 * the user just watched finish; for one that ended while the app was closed,
 * they already got claude-agent's tray reminder, and a burst at startup —
 * possibly before the window is even on screen — would be noise.
 */
export async function reconcilePending(): Promise<{ ideaRel: string; runId: string }[]> {
  const still: { ideaRel: string; runId: string }[] = []
  for (const [ideaRel, runId] of Object.entries(state.pending)) {
    let view: RunView
    try {
      view = interpretStatus(await agentStatus(TASK_ID, runId))
    } catch (e) {
      console.warn('[idea-spark] could not reconcile a pending run (the agent did not answer):', e)
      continue
    }
    if (view.kind === 'running') {
      still.push({ ideaRel, runId })
      continue
    }
    await finishRun(runId, runStatusWord(view))
  }
  clearCelebrate()
  return still
}

/**
 * Saves the editor's markdown. The first save names the file
 * (`YYYY-MM-DD-<slug>.md`, deduped against the directory) and pins it as
 * `current`; every later save overwrites that same file — renaming a document
 * out from under the user because they edited the title would scatter one idea
 * across several files.
 *
 * A blank document (whitespace only) is NOT written: see `isBlank`. This is the
 * "no empty files" rule of the autosave design — every window open would
 * otherwise deposit an empty idea in the inbox.
 *
 * Progress is reported through `saveState` rather than a toast: saving now
 * happens on every pause in typing, and a toast per pause would be unbearable.
 * The action bar renders the state instead (`saving` → `saved HH:mm` →
 * `failed`, the last one clickable to retry). A failure still toasts, because
 * losing writes is worth interrupting for.
 *
 * Returns the saved file's bare name, or null when nothing was written (no
 * vault, blank document, or a failed write).
 */
export async function saveIdea(markdown: string): Promise<string | null> {
  if (!state.vaultRoot) return null
  if (isBlank(markdown)) {
    // Nothing is written — and the previous `saved HH:mm` would now be a claim
    // about content the user has just deleted from the editor. Drop to `idle`
    // so the bar says nothing rather than something false.
    state.saveState = { kind: 'idle' }
    return null
  }
  state.busy = true
  state.saveState = { kind: 'saving' }
  try {
    const name = await freeFileName(markdown)
    const text = ideaDocText(state, markdown, new Date().toISOString())
    await vaultWrite(relPath(state, name), text)
    state.current = name
    // Re-read our own output so the next save preserves this one's `created`.
    state.currentFrontmatter = frontmatterOf(text)
    state.savedMarkdown = markdown
    state.dirty = false
    // The row label comes from the body, and the body just changed. Recomputing
    // it here (from text we already hold) is what keeps an inbox row in step
    // with the heading being typed into it — without it, the cached title would
    // stay whatever the file said when the row first scrolled into view.
    retitle(null, name, rowTitle(text, name))
    await reload()
    state.saveState = { kind: 'saved', at: clockTime(new Date()) }
    return name
  } catch (e) {
    console.error('[idea-spark] save failed:', e)
    state.saveState = { kind: 'failed', message: String(e) }
    toast(String(e), 'error')
    return null
  } finally {
    state.busy = false
  }
}

/**
 * `nextFileName`, then a last-moment check against the disk for a *new* idea.
 *
 * `nextFileName` can only dedupe against what the last listing saw, and that
 * listing can be stale or have failed outright (see `reload`) — in which case
 * it would hand back an un-suffixed name and the write would silently overwrite
 * a same-minute idea (two windows opened within the same 60-second bucket).
 * `host.vault.exists` is the authority, so ask it, and keep asking as long as
 * the answer is "taken" (bounded, so a bridge that answers `true` for
 * everything can't spin forever).
 *
 * An idea that already has a file skips all of this: overwriting itself is the
 * whole point of a second save.
 *
 * Cap behaviour, stated plainly: after 100 occupied candidates the loop gives
 * up and returns the 101st name **unverified**, which could overwrite it. That
 * needs 100 ideas created in the same minute in one directory; a bound is
 * still worth having, because without one a bridge that answered `true` to
 * everything would hang the save (and with it the window) forever.
 */
async function freeFileName(markdown: string): Promise<string> {
  if (state.current) return state.current

  const now = new Date()
  const taken = new Set(fileNames(state))
  let name = nextFileName(state, markdown, now.toISOString())
  for (let i = 0; i < 100; i++) {
    // A failed existence check must not block the save: treat it as free and
    // let the write itself report whatever is really wrong.
    const occupied = await vaultExists(relPath(state, name))
      .then((r) => r.exists)
      .catch(() => false)
    if (!occupied) return name
    taken.add(name)
    name = timestampFileName(now, taken)
  }
  console.warn(`[idea-spark] gave up looking for a free name after 100 tries; using ${name} unchecked`)
  return name
}

/**
 * Loads a saved idea into the editor. Returns the body to display (frontmatter
 * stripped), or null when the read failed — in which case the editor is left
 * untouched rather than blanked.
 */
export async function loadIdea(name: string): Promise<string | null> {
  try {
    const { content } = await vaultRead(relPath(state, name))
    const body = bodyOf(content)
    // The whole document is in hand; refreshing the row label off it costs
    // nothing and corrects a cached title that went stale while the file was
    // being edited somewhere else (see `ensureTitle`).
    retitle(null, name, rowTitle(content, name))
    state.current = name
    state.currentFrontmatter = frontmatterOf(content)
    state.savedMarkdown = body
    state.dirty = false
    // The bar reports the *open* document. Carrying "saved 19:42" over from the
    // idea we just left would claim this one had just been written.
    state.saveState = { kind: 'idle' }
    return body
  } catch (e) {
    console.error('[idea-spark] load failed:', e)
    toast(String(e), 'error')
    return null
  }
}

/**
 * Resets the editor to a fresh, never-saved idea. Returns the (blank) template.
 *
 * Also advances the placeholder rotation and persists it, so the next blank
 * document — this one included, since App.svelte reads `placeholderSeq` when it
 * mounts the editor — shows the next of the five lines instead of reopening on
 * the same one forever. Writing the *state file* here does not contradict "a
 * blank idea is never written": no idea file is created, only the rotation
 * counter moves. `persist()` swallows its own failures, so a read-only vault
 * costs the rotation, never the new document.
 */
export function newIdea(): string {
  const tpl = ideaTemplate()
  state.current = null
  state.currentFrontmatter = null
  state.savedMarkdown = tpl
  state.dirty = false
  // A brand-new blank draft has never been saved, and saying so is the point:
  // otherwise the bar keeps claiming the *previous* idea's save time.
  state.saveState = { kind: 'idle' }
  state.placeholderSeq += 1
  void persist()
  return tpl
}

/** Opens an idea's proof document in the main window's editor. */
export async function openResult(ideaName: string): Promise<void> {
  await openInMain(proofPathFor(relPath(state, ideaName)))
}

/** Opens the idea itself in the main window's editor (the inbox's context menu). */
export async function openIdea(ideaName: string): Promise<void> {
  await openInMain(relPath(state, ideaName))
}

async function openInMain(path: string): Promise<void> {
  try {
    await bridge().request('host.editor.open', { path })
  } catch (e) {
    console.error('[idea-spark] opening a document in the main window failed:', e)
    toast(String(e), 'error')
  }
}

/** Expands/collapses the inbox and remembers the choice for the next window. */
export function toggleInbox(): void {
  state.inboxOpen = !state.inboxOpen
  void persist()
}

/**
 * Names whose body is currently being read. NOT `$state`: it exists purely to
 * keep two rows (or a row that scrolls in and out and back) from asking for the
 * same file twice, and nothing renders it — making it reactive would invalidate
 * every reader of the inbox on each read that starts and finishes.
 */
const titlesInFlight = new Set<string>()

/**
 * Reads one idea's body and caches its row label — the inbox's lazy title
 * hydration, called per row as it scrolls into view.
 *
 * Deliberately one file at a time and only for rows the user can actually see:
 * a directory with a few hundred ideas would otherwise mean a few hundred
 * bridge round trips before the panel could draw anything. Each name is read at
 * most once per window (the cache is only ever invalidated by a save, rename or
 * delete of that same name), and a failed read is silently left uncached — the
 * row keeps the file-name fallback and a later scroll may retry.
 */
/**
 * Drops every cached row label, so the rows on screen read their titles again.
 *
 * The cache is otherwise maintained only by this window's own writes, and this
 * window is not the only writer: "open in the main editor" (the inbox's own
 * context menu) is the shortest path to editing an idea somewhere else, and an
 * agent or a vault sync can rewrite one at any time. Called when the window
 * regains focus — the moment right after the user was plausibly off editing
 * elsewhere — which costs a screenful of reads and nothing for the rows below
 * the fold (they re-hydrate as they scroll past, as always).
 */
export function invalidateTitles(): void {
  state.titles = {}
}

export async function ensureTitle(name: string): Promise<void> {
  if (name in state.titles || titlesInFlight.has(name)) return
  titlesInFlight.add(name)
  try {
    const { content } = await vaultRead(relPath(state, name))
    state.titles = { ...state.titles, [name]: rowTitle(content, name) }
  } catch (e) {
    console.warn('[idea-spark] reading an idea for its inbox title failed:', e)
  } finally {
    titlesInFlight.delete(name)
  }
}

/** Cache maintenance for `titles`: re-key, drop, or overwrite one entry. */
function retitle(from: string | null, to: string | null, title?: string): void {
  const next = { ...state.titles }
  const carried = from === null ? undefined : next[from]
  if (from !== null) delete next[from]
  if (to !== null) {
    const value = title ?? carried
    if (value === undefined) delete next[to]
    else next[to] = value
  }
  state.titles = next
}

/**
 * Deletes an idea and its proof sidecar (`filesToDelete`) — for real, there is
 * no trash. The caller is responsible for having flushed any in-flight save
 * first: a write that lands after the delete would recreate the very file the
 * user just removed.
 *
 * Returns the blank document the caller must show when the idea that was
 * deleted is the one in the editor, and null otherwise. Leaving the deleted
 * text on screen would be worse than cosmetic: `current` still pointed at a
 * file that no longer exists, so the next keystroke's autosave would write it
 * straight back (or, with `current` merely cleared, deposit the same text under
 * a new name). Resetting through `newIdea()` detaches the document and blanks
 * the buffer in one move; only pushing the text into the editor is left, which
 * this layer cannot do.
 *
 * A failed removal is reported and stops the loop — deleting the proof of an
 * idea that is still there would leave the pair worse than it found it — and
 * the listing is refreshed either way, so the panel shows what survived.
 *
 * PARTIAL failure is the case that matters, and it decides the return value:
 * the idea is removed first, so "the sidecar's removal failed" means the idea
 * itself is already gone for good. Treating that as a plain failure — leaving
 * `current` pointing at the file that was just deleted, with its text still in
 * the editor — would resurrect it on the next keystroke: autosave asks
 * `freeFileName`, which hands back `state.current` unchanged, and writes the
 * document the user was told had been deleted for good straight back to disk.
 * So the detach is driven by what actually happened to the IDEA, never by
 * whether the whole batch succeeded.
 */
export async function deleteIdea(name: string): Promise<string | null> {
  const wasOpen = state.current === name
  // The idea is `filesToDelete`'s first entry, so the first successful removal
  // is always the idea itself.
  let ideaGone = false
  state.busy = true
  try {
    for (const path of filesToDelete(state, name)) {
      await vaultRemove(path)
      ideaGone = true
    }
  } catch (e) {
    console.error('[idea-spark] delete failed:', e)
    toast(String(e), 'error')
  } finally {
    state.busy = false
  }
  if (!ideaGone) {
    await reload()
    return null
  }
  retitle(name, null)
  // Every key that names the file has to go with it — the same bookkeeping
  // `renameIdea` does when the name MOVES, except here there is nowhere to
  // move it to. A `pending` entry for a file that no longer exists is not
  // cosmetic: `runInFlight` is a GLOBAL gate (claude-agent serializes the
  // whole task, see its note), so one orphan entry disables delegation for
  // every idea, `persist()` writes it into `.notemd/idea-spark.json` so it
  // survives restarts, and `reconcilePending` deliberately leaves a run it
  // cannot reach alone — leaving hand-editing the JSON as the only way out.
  const ideaRel = relPath(state, name)
  if (ideaRel in state.pending) {
    const { [ideaRel]: _dropped, ...rest } = state.pending
    state.pending = rest
    void persist()
  }
  state.failed = state.failed.filter((f) => f !== ideaRel)
  await reload()
  return wasOpen ? newIdea() : null
}

/**
 * Renames an idea (and its proof sidecar, so the pair stays a pair). Returns
 * false — having changed nothing and told the user why — when the name is
 * refused by `validateRename` or by the host.
 *
 * As with delete, the caller must have flushed any in-flight save first, or it
 * would land on the old name and resurrect it.
 *
 * The sidecar is moved best-effort *after* the idea: if that second call fails
 * the idea is already renamed, and unwinding it would mean a third call that
 * can fail just as easily. The user is told, and what they see is an idea that
 * dropped back to `draft` next to an orphaned `.proof.md` — visible and
 * repairable, which a half-applied rename would not be.
 */
export async function renameIdea(from: string, raw: string): Promise<boolean> {
  const verdict = validateRename(state, from, raw)
  if (!verdict.ok) {
    toast(t(RENAME_ERROR[verdict.reason]), 'error')
    return false
  }
  const to = verdict.name
  if (to === from) return true

  const proof = state.files.includes(proofPathFor(relPath(state, from)))
  state.busy = true
  try {
    await vaultRename(relPath(state, from), relPath(state, to))
  } catch (e) {
    console.error('[idea-spark] rename failed:', e)
    toast(String(e), 'error')
    return false
  } finally {
    state.busy = false
  }

  // Every key that names the file moves with it, and it moves HERE — the
  // instant the idea's own rename lands, before the sidecar's round trip is
  // even started. In between those two `await`s the file simply does not exist
  // under its old name, and `state.current` is what autosave writes to: a
  // change echo arriving in that window (the kit reports edits ~200 ms late)
  // would schedule a write to the old path and, 1.5 s later, recreate the file
  // this rename just moved — leaving two copies of one idea.
  //
  // `pending` and `failed` move for a duller reason: a run in flight keyed on a
  // path that no longer exists loses its ⏳ and never gets its result.
  const fromRel = relPath(state, from)
  const toRel = relPath(state, to)
  if (state.current === from) state.current = to
  if (fromRel in state.pending) {
    const { [fromRel]: runId, ...rest } = state.pending
    state.pending = { ...rest, [toRel]: runId }
    void persist()
  }
  state.failed = state.failed.map((f) => (f === fromRel ? toRel : f))
  retitle(from, to)

  if (proof) {
    try {
      await vaultRename(proofPathFor(fromRel), proofPathFor(toRel))
    } catch (e) {
      console.error('[idea-spark] renaming the proof sidecar failed:', e)
      toast(String(e), 'error')
    }
  }

  await reload()
  return true
}

/** Rejection reasons → the message the user is shown. */
const RENAME_ERROR: Record<'empty' | 'slash' | 'dot' | 'taken', MessageKey> = {
  empty: 'renameEmpty',
  slash: 'renameSlash',
  dot: 'renameDot',
  taken: 'renameTaken',
}

/**
 * Settings: validate, persist and re-list. Returns false (changing nothing)
 * when the directory is rejected, so the popover can keep the field open.
 */
export async function saveIdeaDir(dir: string): Promise<boolean> {
  if (!changeIdeaDir(state, dir)) return false
  state.busy = true
  try {
    await persist()
    await reload()
    return true
  } finally {
    state.busy = false
  }
}

/**
 * Settings' commit gate: change the directory only if `flush` says the open
 * buffer is safe to let go of. Returns false — having changed NOTHING, disk
 * included — otherwise, so the popover stays open on the field the user was
 * editing.
 *
 * The gate exists because `changeIdeaDir` DETACHES the open document
 * (`current`/`currentFrontmatter` cleared): a buffer that never reached the
 * disk would be written by the next autosave tick as a brand-new file, with
 * freshly stamped frontmatter, in the NEW directory — while the original keeps
 * the old text. One idea, silently forked in two. This is the only place where
 * a settings action can produce a file, so it is the only settings action that
 * needs a flush barrier at all.
 *
 * `flush` is a callback rather than a call to `saveIdeaDir`'s own code because
 * the buffer lives in the component tree, not in the store — only App.svelte
 * can ask the live editor what it holds. What it must NOT be is a bare
 * `saveNow()`: that never rejects (`autosave.ts` swallows the write's failure,
 * by design — the caller reports it), so awaiting it proves nothing. The
 * caller has to assert the postcondition — "what the editor holds is on disk"
 * — and say so with a boolean; `App.svelte`'s `keepUnsaved()` is exactly that
 * assertion, and is what gets passed in.
 */
export async function commitIdeaDir(dir: string, flush: () => Promise<boolean>): Promise<boolean> {
  if (!(await flush())) return false
  return await saveIdeaDir(dir)
}

/**
 * Clears the celebration flag once its animation has run its course. Pass the
 * `celebrateSeq` the timer was started for and a stale timer becomes a no-op:
 * burst N's two seconds can never cut burst N+1 short.
 */
export function clearCelebrate(seq?: number): void {
  if (seq !== undefined && seq !== state.celebrateSeq) return
  state.celebrate = false
}
