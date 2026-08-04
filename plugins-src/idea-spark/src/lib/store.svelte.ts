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
import { bridge, vaultExists, vaultInfo, vaultList, vaultRead, vaultWrite } from './bridge'
import { buildIdeaDoc, rebuildIdeaDoc } from './idea-doc'
import { ideaFileName, proofPathFor, splitFrontmatter } from './naming'
import { DEFAULT_STATE, parseState, serializeState, STATE_PATH } from './state-io'
import { deriveStatus, listIdeas, type IdeaStatus } from './status'
import { t } from './strings'

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
   *  empty). An empty history list then means "couldn't read", not "no ideas",
   *  and the save path stops trusting `files` for collision detection. */
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
}

/** Terminal push from the host's run watcher (Task 13 wires the transport). */
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
 * — for an idea that has never been saved — `YYYY-MM-DD-<slug>.md` deduplicated
 * against *every* file in the directory (an orphaned `.proof.md` occupies a
 * name just as much as an idea does).
 *
 * Keeping the name once it exists is deliberate: renaming the document because
 * the user edited its title would scatter one idea across several files.
 *
 * Note this only knows what the last listing saw. `saveIdea` re-checks the
 * winner against the disk before writing — see the note there.
 */
export function nextFileName(s: SparkStore, markdown: string, todayStr: string): string {
  return s.current ?? ideaFileName(markdown, todayStr, new Set(fileNames(s)))
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

/** The pre-filled capture template (localized), used for every fresh idea. */
export function ideaTemplate(): string {
  return [
    `# ${t('templateH1')}`,
    '',
    t('templateHint'),
    '',
    `## ${t('sectionDomain')}`,
    '',
    `## ${t('sectionTransfer')}`,
    '',
    `## ${t('sectionResources')}`,
    '',
    `## ${t('sectionOutcome')}`,
    '',
  ].join('\n')
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

/** Local (not UTC) `YYYY-MM-DD` — the date prefix of a new idea's file name.
 *  `toISOString()` would name a late-evening idea after tomorrow. */
function today(d = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
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
 * that the emptiness is unverified, so the UI can say so and, more importantly,
 * so `saveIdea` stops trusting `files` for collision detection. An empty list
 * that really means "couldn't read" would otherwise make `nextFileName` hand
 * back an un-suffixed name and overwrite a same-day, same-title idea on disk.
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

/** Persists `.notemd/idea-spark.json`. Failure is reported, never thrown. */
async function persist(): Promise<void> {
  try {
    await vaultWrite(STATE_PATH, serializeState({ ideaDir: state.ideaDir, pendingRuns: { ...state.pending } }))
  } catch (e) {
    console.error('[idea-spark] writing plugin state failed:', e)
  }
}

/**
 * Saves the editor's markdown. The first save names the file
 * (`YYYY-MM-DD-<slug>.md`, deduped against the directory) and pins it as
 * `current`; every later save overwrites that same file — renaming a document
 * out from under the user because they edited the title would scatter one idea
 * across several files.
 *
 * Returns the saved file's bare name, or null when the write failed.
 */
export async function saveIdea(markdown: string): Promise<string | null> {
  if (!state.vaultRoot) return null
  state.busy = true
  try {
    const name = await freeFileName(markdown)
    const text = ideaDocText(state, markdown, new Date().toISOString())
    await vaultWrite(relPath(state, name), text)
    state.current = name
    // Re-read our own output so the next save preserves this one's `created`.
    state.currentFrontmatter = frontmatterOf(text)
    state.savedMarkdown = markdown
    state.dirty = false
    await reload()
    toast(t('saved'))
    return name
  } catch (e) {
    console.error('[idea-spark] save failed:', e)
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
 * a same-day, same-title idea. `host.vault.exists` is the authority, so ask it,
 * and keep asking as long as the answer is "taken" (bounded, so a bridge that
 * answers `true` for everything can't spin forever). It also covers a collision
 * the string-level dedup structurally cannot see: the slug keeps the title's
 * case, so `My-Idea.md` and `my-idea.md` are two names but one file on a
 * case-insensitive filesystem (macOS's default).
 *
 * An idea that already has a file skips all of this: overwriting itself is the
 * whole point of a second save.
 */
async function freeFileName(markdown: string): Promise<string> {
  if (state.current) return state.current

  const taken = new Set(fileNames(state))
  let name = nextFileName(state, markdown, today())
  for (let i = 0; i < 100; i++) {
    // A failed existence check must not block the save: treat it as free and
    // let the write itself report whatever is really wrong.
    const occupied = await vaultExists(relPath(state, name))
      .then((r) => r.exists)
      .catch(() => false)
    if (!occupied) break
    taken.add(name)
    name = ideaFileName(markdown, today(), taken)
  }
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
    state.current = name
    state.currentFrontmatter = frontmatterOf(content)
    state.savedMarkdown = body
    state.dirty = false
    return body
  } catch (e) {
    console.error('[idea-spark] load failed:', e)
    toast(String(e), 'error')
    return null
  }
}

/** Resets the editor to a fresh, never-saved idea. Returns the template. */
export function newIdea(): string {
  const tpl = ideaTemplate()
  state.current = null
  state.currentFrontmatter = null
  state.savedMarkdown = tpl
  state.dirty = false
  return tpl
}

/** Opens an idea's proof document in the main window's editor. */
export async function openResult(ideaName: string): Promise<void> {
  const path = proofPathFor(relPath(state, ideaName))
  try {
    await bridge().request('host.editor.open', { path })
  } catch (e) {
    console.error('[idea-spark] opening the result failed:', e)
    toast(String(e), 'error')
  }
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
 * Clears the celebration flag once its animation has run its course. Pass the
 * `celebrateSeq` the timer was started for and a stale timer becomes a no-op:
 * burst N's two seconds can never cut burst N+1 short.
 */
export function clearCelebrate(seq?: number): void {
  if (seq !== undefined && seq !== state.celebrateSeq) return
  state.celebrate = false
}
