import YAML from 'yaml'
import { normalizeVaultDir } from './source'

export const NEXT_PATH = 'thinking/next.note.md'
export const NEXT_TYPE = 'Next'
export const NEXT_VERSION = 1

export interface LedgerDocument {
  type: typeof NEXT_TYPE
  version: typeof NEXT_VERSION
  source_dirs: string[]
  events: Record<string, unknown>[]
  /** Unknown top-level fields survive a read/write cycle. */
  extra: Record<string, unknown>
}

export type LedgerErrorCode =
  | 'missing_frontmatter'
  | 'invalid_yaml'
  | 'invalid_root'
  | 'wrong_type'
  | 'unsupported_version'
  | 'invalid_source_dirs'
  | 'invalid_events'

export class LedgerFormatError extends Error {
  constructor(
    readonly code: LedgerErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'LedgerFormatError'
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function frontmatterText(markdown: string): string {
  const lines = markdown.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') {
    throw new LedgerFormatError('missing_frontmatter', 'Next document has no leading YAML frontmatter')
  }
  const end = lines.findIndex((line, index) => index > 0 && line.trim() === '---')
  if (end < 0) throw new LedgerFormatError('missing_frontmatter', 'Next document has no closing YAML fence')
  return lines.slice(1, end).join('\n')
}

export function parseLedger(markdown: string): LedgerDocument {
  let value: unknown
  try {
    value = YAML.parse(frontmatterText(markdown))
  } catch (error) {
    if (error instanceof LedgerFormatError) throw error
    throw new LedgerFormatError('invalid_yaml', `Next frontmatter is not valid YAML: ${String(error)}`)
  }
  if (!isRecord(value)) throw new LedgerFormatError('invalid_root', 'Next frontmatter must be a mapping')
  if (value.type !== NEXT_TYPE) throw new LedgerFormatError('wrong_type', `Expected type: ${NEXT_TYPE}`)
  if (value.version !== NEXT_VERSION) {
    throw new LedgerFormatError('unsupported_version', `Unsupported Next version: ${String(value.version)}`)
  }
  if (!Array.isArray(value.source_dirs)) {
    throw new LedgerFormatError('invalid_source_dirs', 'Next source_dirs must be an array')
  }
  const sourceDirs: string[] = []
  for (const raw of value.source_dirs) {
    const dir = normalizeVaultDir(raw)
    if (!dir) throw new LedgerFormatError('invalid_source_dirs', 'Next source_dirs contains an unsafe path')
    if (!sourceDirs.includes(dir)) sourceDirs.push(dir)
  }
  if (!Array.isArray(value.events) || !value.events.every(isRecord)) {
    throw new LedgerFormatError('invalid_events', 'Next events must be an array of mappings')
  }
  const { type: _type, version: _version, source_dirs: _sourceDirs, events: _events, ...extra } = value
  return {
    type: NEXT_TYPE,
    version: NEXT_VERSION,
    source_dirs: sourceDirs,
    events: value.events,
    extra,
  }
}

export function newLedger(sourceDirs: string[]): LedgerDocument {
  const normalized = sourceDirs
    .map(normalizeVaultDir)
    .filter((value): value is string => value !== null)
  return {
    type: NEXT_TYPE,
    version: NEXT_VERSION,
    source_dirs: [...new Set(normalized)],
    events: [],
    extra: {},
  }
}

function scalar(value: unknown): string | null {
  if (typeof value !== 'string' || !value.trim()) return null
  return value.replace(/[\r\n]+/g, ' ').trim()
}

function readableBody(events: Record<string, unknown>[]): string {
  const lines = [
    '# Next',
    '',
    '> Managed by Next. The YAML events above are the source of truth; this section is a readable mirror.',
    '',
  ]
  if (events.length === 0) {
    lines.push('No ideas have been placed yet.', '')
    return `${lines.join('\n')}\n`
  }
  for (const event of events.slice().reverse()) {
    const at = scalar(event.at) ?? 'Unknown time'
    const action = scalar(event.action) ?? 'invalid event'
    lines.push(`## ${at} · ${action}`)
    const ideaId = scalar(event.idea_id)
    const source = isRecord(event.source) ? scalar(event.source.path) : null
    if (ideaId) lines.push(`- idea: \`${ideaId}\``)
    if (source) lines.push(`- source: \`${source}\``)
    for (const [key, label] of [
      ['commitment', 'commitment'],
      ['next_action', 'next action'],
      ['close_condition', 'close condition'],
      ['waiting_for', 'waiting for'],
      ['review_at', 'review on'],
      ['wake_trigger', 'wake trigger'],
      ['project', 'project'],
      ['reason', 'reason'],
      ['target', 'destination'],
      ['result', 'result'],
    ] as const) {
      const value = scalar(event[key])
      if (value) lines.push(`- ${label}: ${value}`)
    }
    if (isRecord(event.exit)) {
      const kind = scalar(event.exit.kind)
      const via = scalar(event.exit.via)
      const delivery = scalar(event.exit.delivery)
      if (kind) lines.push(`- outcome: ${kind}${via ? ` via ${via}` : ''}${delivery ? ` · delivery: ${delivery}` : ''}`)
    }
    lines.push('')
  }
  return `${lines.join('\n')}\n`
}

export function serializeLedger(document: LedgerDocument): string {
  const frontmatter = YAML.stringify({
    type: NEXT_TYPE,
    version: NEXT_VERSION,
    ...document.extra,
    source_dirs: [...new Set(document.source_dirs)],
    events: document.events,
  }).trimEnd()
  return `---\n${frontmatter}\n---\n\n${readableBody(document.events)}`
}
