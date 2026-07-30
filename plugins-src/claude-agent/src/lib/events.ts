// Backend event stream → view model. Kept a pure function so it's testable;
// the Svelte side only renders what comes out of here.
export type Item =
  | { type: 'text'; text: string }
  | { type: 'tool'; name: string; brief: string }

export type Status =
  | 'idle'
  | 'running'
  | 'success'
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
}

export function emptyView(): RunView {
  return { runId: null, status: 'idle', items: [] }
}

/** One row of `runs/<runId>.json`, as the backend serializes it. */
export interface RunRecord {
  run_id: string
  task: string
  trigger: 'window' | 'cli'
  started_at: string
  ended_at: string
  status: Status
  num_turns?: number | null
  result: string
  stderr_tail: string
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
}

type BackendEvent =
  | { kind: 'text'; text: string }
  | { kind: 'tool_use'; name: string; brief: string }
  | { kind: 'system'; subtype: string }
  | { kind: 'result'; [k: string]: unknown }

/** The envelopes the backend pushes through `host.ui.post`. */
export type HostMessage =
  | { kind: 'event'; run_id: string; event: BackendEvent }
  | {
      kind: 'done'
      run_id: string
      record: { status: Status; num_turns?: number; result?: string }
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
  return view
}
