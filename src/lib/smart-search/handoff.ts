import type { SearchHit } from '../search/api'
import type { ResolvedSearchPlan } from './plan'

export interface HandoffRef {
  path: string
  line: number
  lineEnd: number
}

export interface HandoffPacket {
  version: 1
  question: string
  resolvedFilters: Record<string, unknown>
  queryTerms: string[]
  selectedRefs: HandoffRef[]
  limitations: ['lookup_results_are_not_complete_evidence']
}

const MAX_REFS = 20
const MAX_PACKET_BYTES = 16 * 1024

function validRelativePath(path: string): boolean {
  if (!path || path.length > 2_048 || path.includes('\0') || path.includes('\\') || path.startsWith('/')) return false
  return path.split('/').every((part) => part.length > 0 && part !== '.' && part !== '..')
}

function safeFilterValue(value: unknown, depth = 0): unknown {
  if (depth > 6) return undefined
  if (value === null || typeof value === 'boolean' || typeof value === 'number') return value
  if (typeof value === 'string') {
    const text = value.trim()
    if (!text || text.includes('\0') || text.includes('\\') || text.startsWith('/') || /^[A-Za-z]:[\\/]/.test(text)) {
      return undefined
    }
    return text.slice(0, 512)
  }
  if (Array.isArray(value)) {
    return value.map((item) => safeFilterValue(item, depth + 1)).filter((item) => item !== undefined)
  }
  if (typeof value === 'object') {
    const result: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(value as Record<string, unknown>).slice(0, 64)) {
      if (!key || key.length > 128 || key.includes('\0')) continue
      const safe = safeFilterValue(item, depth + 1)
      if (safe !== undefined) result[key] = safe
    }
    return result
  }
  return undefined
}

export function buildHandoffPacket(
  question: string,
  plan: ResolvedSearchPlan | null,
  hits: SearchHit[],
): HandoffPacket {
  const selectedRefs = hits
    .filter((hit) => validRelativePath(hit.path) && hit.line > 0 && hit.lineEnd >= hit.line)
    .slice(0, MAX_REFS)
    .map((hit) => ({ path: hit.path, line: hit.line, lineEnd: hit.lineEnd }))
  const queryTerms = Array.from(new Set(
    plan?.queries.flatMap((query) => [...query.terms, ...query.phrases]) ?? [],
  )).map((term) => term.trim()).filter((term) => (
    term.length > 0
      && Array.from(term).length <= 256
      && !term.includes('\0')
      && !term.includes('\\')
      && !term.startsWith('/')
      && !/^[A-Za-z]:[\\/]/.test(term)
  )).slice(0, 24)
  const resolvedFilters: Record<string, unknown> = {}
  if (plan) {
    for (const key of ['paths', 'tags', 'types', 'extensions', 'origins', 'linkedPages'] as const) {
      const values = Array.from(new Set(plan.queries.flatMap((query) => (
        Array.isArray(query.filters[key]) ? query.filters[key] as string[] : []
      ))))
      const safe = safeFilterValue(values)
      if (Array.isArray(safe) && safe.length) resolvedFilters[key] = safe
    }
    // Query filters are the authoritative executed range: the host intersects
    // Planner time with user-locked after/before before building these arms.
    // `plan.time` is only a fallback for older hosts that did not expose the
    // effective range there.
    const after = plan.queries.find((query) => typeof query.filters.after === 'string')?.filters.after
      ?? plan.time?.after
    const before = plan.queries.find((query) => typeof query.filters.before === 'string')?.filters.before
      ?? plan.time?.before
    if (typeof after === 'string') resolvedFilters.after = after
    if (typeof before === 'string') resolvedFilters.before = before
  }
  if (plan?.sort && plan.sort !== 'relevance') resolvedFilters.sort = plan.sort
  const packet: HandoffPacket = {
    version: 1,
    question: Array.from(question.trim()).slice(0, 2_000).join(''),
    resolvedFilters,
    queryTerms,
    selectedRefs,
    limitations: ['lookup_results_are_not_complete_evidence'],
  }
  if (new TextEncoder().encode(JSON.stringify(packet)).length > MAX_PACKET_BYTES) {
    packet.resolvedFilters = {}
    while (packet.queryTerms.length > 0
      && new TextEncoder().encode(JSON.stringify(packet)).length > MAX_PACKET_BYTES) {
      packet.queryTerms.pop()
    }
    while (packet.selectedRefs.length > 0
      && new TextEncoder().encode(JSON.stringify(packet)).length > MAX_PACKET_BYTES) {
      packet.selectedRefs.pop()
    }
  }
  return packet
}

export function buildHandoffPrompt(packet: HandoffPacket): string {
  return [
    '回答用户问题。先根据根 AGENTS.md 确认 Vault 约定；使用 notemd search 验证并扩展候选来源。',
    '需要个人或项目长期上下文时，按当前 Agent 身份、Role、Scope 和 purpose=information-answer 调用 notemd memory context；不要使用未经 context broker 允许的 Memory。',
    '下面的 refs 只是检索起点，不是完整证据；请自行重搜、重读并限定结论。',
    '',
    'HANDOFF_PACKET_JSON',
    JSON.stringify(packet),
  ].join('\n')
}
