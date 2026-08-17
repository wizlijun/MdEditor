import { invoke } from '@tauri-apps/api/core'
import { pluginRuntime } from '../plugins/runtime.svelte'

/**
 * The Agent workspace under the sidecar-note panel: hand ONE note to the
 * claude-agent plugin and watch it work.
 *
 * Progress can't be pushed to us — `host.ui.post` only reaches a plugin's own
 * window, and the run may even belong to a detached CLI process — so we poll
 * the plugin's `run-status`, which reads the lock, the progress snapshot and
 * the run record off disk.
 */
/// The commands the agent slot dispatches. A plugin that activates on all three
/// can serve it — the same convention the host uses
/// (src-tauri/src/plugin_runtime/agent_provider.rs). Deliberately NOT a new
/// manifest field: `contributes` is deny_unknown_fields, so adding one would
/// stop older hosts loading the plugin at all.
export const AGENT_COMMANDS = ['run-task', 'run-note', 'run-status'] as const
/** Used when nothing is configured — what every existing vault already has. */
export const DEFAULT_PLUGIN_ID = 'notemd.claude-agent'
/** @deprecated Use `agentProviders()` / `activeProvider()`; kept for callers not yet updated. */
export const PLUGIN_ID = DEFAULT_PLUGIN_ID
export const NOTE_TASK = 'answer-note-question'
export const POLL_MS = 1000

export type AgentPhase = 'idle' | 'starting' | 'running' | 'done' | 'error'

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
  const ids = pluginRuntime.manifests
    .filter((m) => {
      const events: string[] = (m as { activation?: { events?: string[] } }).activation?.events ?? []
      return AGENT_COMMANDS.every((c) => events.includes(`onCommand:${c}`))
    })
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
  if (chosenProvider && installed.includes(chosenProvider)) return chosenProvider
  if (installed.includes(DEFAULT_PLUGIN_ID)) return DEFAULT_PLUGIN_ID
  return installed[0] ?? DEFAULT_PLUGIN_ID
}

/** Point the workspace at a different harness. Ignored while a run is in flight. */
export function setProvider(id: string): void {
  if (isAgentBusy()) return
  chosenProvider = id
}

let chosenProvider: string | null = null

/** Is any agent plugin installed and enabled? */
export function agentPluginAvailable(): boolean {
  return agentProviders().length > 0
}

type Execute = (command: string, context: unknown) => Promise<any>

// Resolved per call, not captured once: switching provider mid-session must
// take effect on the next run rather than at the next restart.
const dispatch: Execute = (command, context) =>
  invoke('plugin_v2_execute', { pluginId: activeProvider(), command, context })

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
