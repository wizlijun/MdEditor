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
import { bridge, vaultInfo, vaultList, vaultRead, vaultWrite } from './bridge'
import { buildIdeaDoc, rebuildIdeaDoc } from './idea-doc'
import { ideaFileName, proofPathFor, stripLeadingFrontmatter } from './naming'
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

  const proof = ev.open_path ?? proofPathFor(ideaRel)
  if (!s.files.includes(proof)) s.files = [...s.files, proof]
  s.failed = s.failed.filter((f) => f !== ideaRel)
  s.lastResult = proof
  s.celebrate = true
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
  return stripLeadingFrontmatter(md).replace(/^\n+/, '')
}

/** `2026-08-04-my-idea.md` → `my-idea` — the history list's label. */
export function displayName(name: string): string {
  const base = name.endsWith('.md') ? name.slice(0, -3) : name
  const stripped = base.replace(/^\d{4}-\d{2}-\d{2}-/, '')
  return stripped || base
}

/**
 * The raw YAML of a leading frontmatter block (fences excluded), or null when
 * the document has none / never closes the one it opens — the same "what counts
 * as frontmatter" rule `stripLeadingFrontmatter` applies to the body side.
 * Line endings are normalized to `\n` so a CRLF file round-trips cleanly.
 */
export function frontmatterOf(md: string): string | null {
  const lines = md.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') return null
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') return lines.slice(1, i).join('\n')
  }
  return null
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

/** Re-lists the idea directory. A missing directory is simply empty. */
export async function reload(): Promise<void> {
  const entries = await vaultList(state.ideaDir)
    .then((r) => r.entries ?? [])
    .catch(() => [] as { name: string; is_dir: boolean }[])
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
    // Dedup against every file in the directory, not just the ideas: a name
    // that collides with, say, an orphaned `.proof.md` is still a collision.
    const name = state.current ?? ideaFileName(markdown, today(), new Set(fileNames()))
    const now = new Date().toISOString()
    const text =
      state.currentFrontmatter === null
        ? buildIdeaDoc(markdown, now)
        : rebuildIdeaDoc(state.currentFrontmatter, markdown, now)
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

/** Bare names of every known file (idea + sidecars), for dedup on first save. */
function fileNames(): string[] {
  const prefix = `${state.ideaDir}/`
  return state.files.map((f) => (f.startsWith(prefix) ? f.slice(prefix.length) : f))
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
  const before = state.ideaDir
  if (!setIdeaDir(state, dir)) return false
  if (state.ideaDir !== before) {
    // The open document lives in the old directory and stays there. Detaching
    // it means the next save creates a fresh, properly deduplicated file in the
    // new directory instead of silently cloning the old name into it.
    state.current = null
    state.currentFrontmatter = null
  }
  state.busy = true
  try {
    await persist()
    await reload()
    return true
  } finally {
    state.busy = false
  }
}

/** Clears the celebration flag once its animation has run its course. */
export function clearCelebrate(): void {
  state.celebrate = false
}
