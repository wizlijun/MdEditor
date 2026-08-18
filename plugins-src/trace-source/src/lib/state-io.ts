// Pure (de)serialization for `.notemd/trace-source.json` (same pattern as
// idea-spark's state-io.ts). Actual reads/writes go through the host bridge —
// App-layer concern, not this module's. `parseState` must never throw: a
// corrupt or partially written config degrades to defaults, not to a crash.

export interface TraceState {
  /** Vault-relative directory trace reports are written into. */
  traceDir: string
  /** Whether the inbox panel is expanded. */
  inboxOpen: boolean
}

export const DEFAULT_STATE: TraceState = {
  traceDir: 'inbox/traces',
  inboxOpen: false,
}

export const STATE_PATH = '.notemd/trace-source.json'

function defaultState(): TraceState {
  return { traceDir: DEFAULT_STATE.traceDir, inboxOpen: DEFAULT_STATE.inboxOpen }
}

/**
 * Parses `.notemd/trace-source.json` content. Tolerant of everything: `null`
 * (file doesn't exist yet), empty string, unparseable JSON, JSON that isn't an
 * object, and individual keys that are missing or the wrong type — each bad
 * key falls back to its default independently.
 */
export function parseState(raw: string | null): TraceState {
  if (!raw) return defaultState()
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return defaultState()
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return defaultState()
  const o = parsed as Record<string, unknown>
  const traceDir =
    typeof o.traceDir === 'string' && o.traceDir.trim() !== '' ? o.traceDir : DEFAULT_STATE.traceDir
  const inboxOpen = o.inboxOpen === true
  return { traceDir, inboxOpen }
}

export function serializeState(s: TraceState): string {
  return JSON.stringify(s, null, 2)
}

/** Same rules as idea-spark's `normalizeIdeaDir`: vault-relative only. */
export function normalizeTraceDir(dir: string): string | null {
  const trimmed = dir.trim()
  if (trimmed.startsWith('/')) return null
  if (trimmed.includes('..')) return null
  return trimmed.replace(/\/+$/, '') || null
}
