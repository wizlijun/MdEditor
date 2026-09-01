export const PRIORITIES = ['P0', 'P1', 'P2', 'P3'] as const
export type Priority = (typeof PRIORITIES)[number]

export const DEFAULT_PRIORITY: Priority = 'P2'

export interface PlanningMetadata {
  priority: Priority
  due?: string
  contexts: string[]
}

export function normalizePriority(value: unknown): Priority {
  return typeof value === 'string' && (PRIORITIES as readonly string[]).includes(value)
    ? value as Priority
    : DEFAULT_PRIORITY
}

export function isCalendarDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value)
  if (!match) return false
  const date = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])))
  return !Number.isNaN(date.valueOf()) && date.toISOString().slice(0, 10) === value
}

export function normalizeDue(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  const due = value.trim()
  return isCalendarDate(due) ? due : undefined
}

export function normalizeContexts(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  const contexts: string[] = []
  const keys = new Set<string>()
  for (const candidate of value) {
    if (typeof candidate !== 'string') continue
    const context = candidate.normalize('NFKC').trim()
    const key = context.toLocaleLowerCase()
    if (!context || keys.has(key)) continue
    contexts.push(context)
    keys.add(key)
  }
  return contexts
}

export function parseContextDraft(value: string): string[] {
  return normalizeContexts(value.split(/[,，\n]/u))
}

export function localDateAfter(days: number, now = new Date()): string {
  const date = new Date(now)
  date.setHours(12, 0, 0, 0)
  date.setDate(date.getDate() + days)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}
