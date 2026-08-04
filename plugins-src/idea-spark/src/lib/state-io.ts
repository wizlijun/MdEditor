// Pure (de)serialization for `.notemd/idea-spark.json`. Actual reads/writes
// go through the host bridge (host.vault.read/write) — App-layer concern,
// not this module's. `parseState` must never throw: a corrupt or partially
// written config file should degrade to defaults, not crash the plugin
// window on open.

export interface SparkState {
  /** Vault-relative directory ideas are saved into. */
  ideaDir: string
  /** ideaRelPath → run_id, for ideas currently being argued by claude-agent. */
  pendingRuns: Record<string, string>
  /** Whether the inbox panel is expanded. */
  inboxOpen: boolean
  /** Rotation counter for the blank-document placeholder line (`placeholder.ts`). */
  placeholderSeq: number
}

export const DEFAULT_STATE: SparkState = {
  ideaDir: 'inbox/ideas',
  pendingRuns: {},
  inboxOpen: false,
  placeholderSeq: 0,
}

export const STATE_PATH = '.notemd/idea-spark.json'

/** Fresh copy of the defaults — never hand out `DEFAULT_STATE` itself, since
 *  `pendingRuns` is a mutable object callers might write into. */
function defaultState(): SparkState {
  return {
    ideaDir: DEFAULT_STATE.ideaDir,
    pendingRuns: {},
    inboxOpen: DEFAULT_STATE.inboxOpen,
    placeholderSeq: DEFAULT_STATE.placeholderSeq,
  }
}

function isStringRecord(v: unknown): v is Record<string, string> {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) return false
  return Object.values(v).every((x) => typeof x === 'string')
}

/**
 * Parses `.notemd/idea-spark.json` content. Tolerant of everything: `null`
 * (file doesn't exist yet), empty string, unparseable JSON, JSON that isn't
 * an object, and individual keys that are missing or the wrong type — each
 * bad/missing key falls back to its default independently rather than
 * discarding the whole object.
 */
export function parseState(raw: string | null): SparkState {
  if (!raw) return defaultState()

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return defaultState()
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return defaultState()

  const o = parsed as Record<string, unknown>
  const ideaDir = typeof o.ideaDir === 'string' && o.ideaDir.trim() !== '' ? o.ideaDir : DEFAULT_STATE.ideaDir
  const pendingRuns = isStringRecord(o.pendingRuns) ? o.pendingRuns : {}
  const inboxOpen = o.inboxOpen === true
  const placeholderSeq = typeof o.placeholderSeq === 'number' && Number.isFinite(o.placeholderSeq) ? o.placeholderSeq : 0
  return { ideaDir, pendingRuns, inboxOpen, placeholderSeq }
}

export function serializeState(s: SparkState): string {
  return JSON.stringify(s, null, 2)
}
