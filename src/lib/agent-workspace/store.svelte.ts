import { invoke } from '@tauri-apps/api/core'
import { pluginRuntime } from '../plugins/runtime.svelte'
import { rememberProvider, rememberedProvider } from '../agent-picker/types'

/**
 * The Agent workspace under the sidecar-note panel: hand ONE note to the
 * claude-agent plugin and watch it work.
 *
 * Progress can't be pushed to us — `host.ui.post` only reaches a plugin's own
 * window, and the run may even belong to a detached CLI process — so we poll
 * the plugin's `run-status`, which reads the lock, the progress snapshot and
 * the run record off disk.
 */
/** Used when nothing is configured — what every existing vault already has. */
export const DEFAULT_PLUGIN_ID = 'notemd.claude-agent'
/** @deprecated Use `agentProviders()` / `activeProvider()`; kept for callers not yet updated. */
export const PLUGIN_ID = DEFAULT_PLUGIN_ID
export const NOTE_TASK = 'answer-note-question'
export const POLL_MS = 1000

export type AgentPhase = 'idle' | 'starting' | 'running' | 'done' | 'error'

/**
 * One agent's harness, as its plugin reports it (`harness-status`).
 *
 * Version and model are not decoration: with two agents installed, "which one
 * am I about to spend tokens on, and is it even working" is exactly what the
 * switcher has to answer BEFORE the click.
 */
export interface HarnessStatus {
  /** The harness's own name: "Claude Code", "DeepSeek Harness". */
  harness: string
  /** Is the executable there? Everything else is decoration if this is false. */
  ok: boolean
  version?: string | null
  /** Where it resolved from — a path, or "monorepo checkout at …". */
  origin?: string
  /** The model used when the task pins none. */
  default_model?: string | null
  /** What to do about it when `ok` is false. */
  hint?: string | null
  /** An environment-level failure seen in the newest run — expired credentials,
   *  rate limits. The run would fail the same way again. */
  warning?: string | null
}

export interface AgentRunState {
  phase: AgentPhase
  /** Which note this run is about (absolute path). */
  notePath: string | null
  runId: string | null
  /** Steps the run has taken — tool calls and replies. */
  steps: number
  /** Newest activity, e.g. "Read a.note.md". */
  last: string
  startedAt: number | null
  /** Terminal status from the run record: success | error | timeout | cancelled. */
  outcome: string | null
  /** The run's final answer, or the reason it failed. */
  message: string
  /** Vault-relative markdown the run produced. */
  artifacts: string[]
  /** Which agent plugin served this run. */
  provider: string | null
}

export function emptyRun(): AgentRunState {
  return {
    phase: 'idle',
    notePath: null,
    runId: null,
    steps: 0,
    last: '',
    startedAt: null,
    outcome: null,
    message: '',
    artifacts: [],
    provider: null,
  }
}

export const agentRun = $state<AgentRunState>(emptyRun())

/** The outline must not be edited underneath a run that is rewriting it. */
export function isAgentBusy(): boolean {
  return agentRun.phase === 'starting' || agentRun.phase === 'running'
}

/** Every installed plugin that can serve the agent slot, default first. */
export function agentProviders(): string[] {
  // `agent_provider` is computed host-side and projected by the adapter. Do NOT
  // re-derive it here from `activation.events`: the view model carries no
  // `activation` at all, so that check matches nothing and the Agent area
  // silently disappears (the v6.817.4 regression).
  const ids = pluginRuntime.manifests
    .filter((m) => m.agent_provider === true)
    .map((m) => m.id)
    .sort()
  // Default first, so the one that will actually run is the one read first.
  const i = ids.indexOf(DEFAULT_PLUGIN_ID)
  if (i > 0) {
    ids.splice(i, 1)
    ids.unshift(DEFAULT_PLUGIN_ID)
  }
  return ids
}

/** Which provider this workspace dispatches to. */
export function activeProvider(): string {
  const installed = agentProviders()
  if (chosen.id && installed.includes(chosen.id)) return chosen.id
  if (installed.includes(DEFAULT_PLUGIN_ID)) return DEFAULT_PLUGIN_ID
  return installed[0] ?? DEFAULT_PLUGIN_ID
}

/** Point the workspace at a different harness. Ignored while a run is in flight. */
export function setProvider(id: string): void {
  if (isAgentBusy()) return
  chosen.id = id
  rememberProvider(SURFACE, id)
}

/**
 * The chosen provider, as reactive state.
 *
 * It has to be `$state`: a plain module variable changes without telling
 * anybody, so the picker's tick and its "by X" label kept rendering the old
 * agent after a choice — which reads as "selecting DeepSeek did nothing", even
 * though the next run would have gone there.
 *
 * A field on an object rather than a bare `let`, because a module-level `$state`
 * primitive is copied at import and the reassignment would not travel.
 */
const chosen = $state<{ id: string | null }>({ id: null })

/** This surface's name for the per-surface memory the plugins also use. */
const SURFACE = 'note'

/** Restore the last choice. Safe to call repeatedly. */
export function restoreProvider(): void {
  const installed = agentProviders()
  if (!installed.length) return
  chosen.id = rememberedProvider(SURFACE, installed, DEFAULT_PLUGIN_ID)
}

/** provider id → its harness, once asked. */
export const harnessStatuses = $state<Record<string, HarnessStatus>>({})

/**
 * Ask every installed provider about its harness.
 *
 * Each answer shells out to `<harness> --version`, so this is called when the
 * panel appears and after a run ends (a run is exactly when an expired
 * credential becomes visible) — not on every render.
 */
export async function refreshHarnesses(): Promise<void> {
  await Promise.all(
    agentProviders().map(async (id) => {
      try {
        const s = await invoke<HarnessStatus>('plugin_v2_execute', {
          pluginId: id,
          command: 'harness-status',
          context: {},
        })
        if (s && typeof s === 'object') harnessStatuses[id] = s
      } catch (e) {
        // A plugin too old to answer, or one that failed to start. Say so
        // rather than leaving the row blank — "unknown" is information.
        harnessStatuses[id] = {
          harness: id,
          ok: false,
          hint: e instanceof Error ? e.message : String(e),
        }
      }
    }),
  )
}

/** Is any agent plugin installed and enabled? */
export function agentPluginAvailable(): boolean {
  return agentProviders().length > 0
}

type Execute = (command: string, context: unknown) => Promise<any>

// Resolved per call, not captured once: switching provider mid-session must
// take effect on the next run rather than at the next restart.
const dispatch: Execute = (command, context) =>
  invoke('plugin_v2_execute', {
    // A run in flight keeps polling the agent that STARTED it. Reading the
    // picker here instead would send `run-status` to the other plugin the
    // moment the choice changed — which reports `lost`, because that plugin
    // never ran anything with this id.
    pluginId: agentRun.provider ?? activeProvider(),
    command,
    context,
  })

let execute: Execute = dispatch

/** Test seam: swap the plugin transport. */
export function __setExecuteForTests(fn: Execute | null): void {
  execute = fn ?? dispatch
}

let timer: ReturnType<typeof setTimeout> | null = null

function stopPolling() {
  if (timer != null) {
    clearTimeout(timer)
    timer = null
  }
}

/**
 * Start answering the open questions in `notePath`.
 * `onFinished` fires once, after a terminal state, so the caller can refresh
 * the views the run just rewrote.
 */
export async function startNoteRun(
  notePath: string,
  onFinished: () => void | Promise<void>,
): Promise<void> {
  if (isAgentBusy()) return
  stopPolling()
  Object.assign(agentRun, emptyRun(), {
    phase: 'starting' as AgentPhase,
    notePath,
    startedAt: Date.now(),
    provider: activeProvider(),
  })
  try {
    const r = await execute('run-note', { note_path: notePath, task: NOTE_TASK })
    const runId = r?.run_id
    if (typeof runId !== 'string' || !runId) throw new Error('the plugin returned no run id')
    agentRun.runId = runId
    agentRun.phase = 'running'
    schedulePoll(onFinished)
  } catch (e) {
    fail(e)
  }
}

function schedulePoll(onFinished: () => void | Promise<void>) {
  stopPolling()
  timer = setTimeout(() => void poll(onFinished), POLL_MS)
}

async function poll(onFinished: () => void | Promise<void>) {
  if (!agentRun.runId) return
  try {
    const s = await execute('run-status', { run_id: agentRun.runId, task: NOTE_TASK })
    switch (s?.state) {
      case 'running':
        agentRun.steps = s.steps ?? 0
        agentRun.last = s.last ?? ''
        schedulePoll(onFinished)
        return
      case 'done': {
        const rec = s.record ?? {}
        agentRun.outcome = rec.status ?? 'success'
        // A skipped run is a fine outcome: the precheck found nothing to do
        // and spent no tokens saying so.
        agentRun.phase = rec.status === 'success' || rec.status === 'skipped' ? 'done' : 'error'
        agentRun.message = rec.result || rec.stderr_tail || ''
        agentRun.artifacts = rec.artifacts ?? []
        stopPolling()
        await onFinished()
        return
      }
      default:
        // 'lost': the run ended without leaving a record — a crash, or a
        // process reaped mid-run. Say so rather than polling forever.
        agentRun.phase = 'error'
        agentRun.outcome = 'lost'
        agentRun.message = 'the run ended without leaving a record'
        stopPolling()
        await onFinished()
    }
  } catch (e) {
    fail(e)
    await onFinished()
  }
}

function fail(e: unknown) {
  stopPolling()
  agentRun.phase = 'error'
  agentRun.outcome = 'error'
  agentRun.message = e instanceof Error ? e.message : String(e)
}

/** Clear a finished run's result banner. No-op while one is in flight. */
export function dismissRun(): void {
  if (isAgentBusy()) return
  stopPolling()
  Object.assign(agentRun, emptyRun())
}
