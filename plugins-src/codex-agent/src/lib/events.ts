// Backend event stream → view model. Kept a pure function so it's testable;
// the Svelte side only renders what comes out of here.
export type Item =
  | { type: 'text'; text: string }
  | { type: 'tool'; name: string; brief: string }
  // A permission decision reported by the harness, when available.
  | { type: 'permission'; tool: string; decision: string }

export type Status =
  | 'idle'
  | 'running'
  | 'success'
  | 'skipped'
  | 'error'
  | 'timeout'
  | 'cancelled'
  | 'busy'

export interface RunView {
  runId: string | null
  status: Status
  items: Item[]
  turns?: number
  result?: string
  /** Vault-relative markdown the finished run produced. */
  artifacts: string[]
  usage?: Usage | null
}

export function emptyView(): RunView {
  return { runId: null, status: 'idle', items: [], artifacts: [] }
}

export interface UsageCost {
  amount_usd: number
  kind: 'provider_reported' | 'list_price_estimate'
  pricing_as_of?: string | null
}

export interface Usage {
  model?: string | null
  input_tokens: number
  cache_read_tokens: number
  cache_write_tokens: number
  output_tokens: number
  reasoning_tokens: number
  reported_total_tokens: number
  cost?: UsageCost | null
}

export function usageTotal(usage: Usage): number {
  return usage.reported_total_tokens > 0
    ? usage.reported_total_tokens
    : usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens + usage.output_tokens
}

export function hasTokenUsage(usage: Usage): boolean {
  return usageTotal(usage) > 0
}

/** One row of `runs/<runId>.json`, as the backend serializes it. */
export interface RunRecord {
  run_id: string
  task: string
  trigger: 'window' | 'cli' | 'note' | 'relay'
  started_at: string
  ended_at: string
  status: Status
  /** Optional because not every Codex result reports a turn count. */
  num_turns?: number | null
  /** The harness's own session id — the pointer back into its session log. */
  session_id?: string | null
  result: string
  stderr_tail: string
  /** Vault-relative markdown this run produced; opens in an editor tab. */
  artifacts?: string[]
  /** WHICH agent plugin performed this run. Both agents share one runs root, so
   *  a row without this is from before the field existed — unknown, not ours. */
  harness?: string | null
  usage?: Usage | null
}

/** A task template plus its live state (`tasks.list`). */
export interface Task {
  id: string
  name: string
  description: string
  /** Read off the lock file, so a detached CLI run shows up here too. */
  running: boolean
  running_since?: string | null
  last_run?: RunRecord | null
  /** The Codex sandbox preset this task runs under. */
  permission_mode?: string
  /** Why the task's author chose that mode. */
  policy_rationale?: string
  /** A malformed policy is fail-closed and must not be presented as a default. */
  policy_error?: string | null
}

type BackendEvent =
  | { kind: 'text'; text: string }
  | { kind: 'tool_use'; name: string; brief: string }
  | { kind: 'permission'; tool: string; decision: string }
  | { kind: 'system'; subtype: string }
  | { kind: 'result'; [k: string]: unknown }

/** The envelopes the backend pushes through `host.ui.post`. */
export type HostMessage =
  | { kind: 'event'; run_id: string; event: BackendEvent }
  | {
      kind: 'done'
      run_id: string
      record: { status: Status; num_turns?: number; result?: string; artifacts?: string[]; usage?: Usage | null }
    }
  | { kind: 'busy'; run_id: string; holder: unknown }

export function reduce(view: RunView, msg: HostMessage): RunView {
  // Leftovers from a previous run must not pollute the current view. Before
  // run.start resolves we don't know the id yet, so accept anything.
  if (view.runId && msg.run_id !== view.runId) return view

  if (msg.kind === 'busy') return { ...view, status: 'busy' }

  if (msg.kind === 'done') {
    return {
      ...view,
      status: msg.record.status,
      turns: msg.record.num_turns,
      result: msg.record.result,
      artifacts: msg.record.artifacts ?? [],
      usage: msg.record.usage,
    }
  }

  const e = msg.event
  if (e.kind === 'text') {
    const last = view.items[view.items.length - 1]
    // Streamed text arrives in fragments; merging keeps it from shattering
    // into dozens of rows.
    const items: Item[] =
      last?.type === 'text'
        ? [...view.items.slice(0, -1), { type: 'text', text: last.text + e.text }]
        : [...view.items, { type: 'text', text: e.text }]
    return { ...view, items }
  }
  if (e.kind === 'tool_use') {
    return { ...view, items: [...view.items, { type: 'tool', name: e.name, brief: e.brief }] }
  }
  if (e.kind === 'permission') {
    return {
      ...view,
      items: [...view.items, { type: 'permission', tool: e.tool, decision: e.decision }],
    }
  }
  // An unknown kind is ignored rather than fatal: the backend may add variants
  // (it only ever adds), and an older window must keep rendering.
  return view
}

/** One harness, as the backend's `harness-status` command reports it. */
export interface HarnessStatus {
  /** The harness's own name, for example "Codex CLI". */
  harness: string
  /** Is the executable there? Everything else is decoration if this is false. */
  ok: boolean
  reason?: 'missing' | 'not_logged_in' | 'probe_failed'
  version?: string | null
  /** Where the executable resolved from. */
  origin?: string
  /** The model used when the task pins none. */
  default_model?: string | null
  hint?: string | null
  /** An environment-level failure seen in the newest run — expired credentials,
   *  rate limits. The run would fail the same way again, whatever the task. */
  warning?: string | null
}
