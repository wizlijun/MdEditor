import type { HemoryDetection, HemoryUser, MigrationAction, MigrationReport } from './types'

export function normalizeUsers(detection: HemoryDetection): HemoryUser[] {
  const seen = new Set<string>()
  const out: HemoryUser[] = []
  for (const value of detection.users ?? []) {
    const user = typeof value === 'string'
      ? { id: value, label: value }
      : { id: value.id, label: value.label || value.id }
    if (!user.id || seen.has(user.id)) continue
    seen.add(user.id)
    out.push(user)
  }
  return out
}

export function initialUser(detection: HemoryDetection): string {
  const users = normalizeUsers(detection)
  if (detection.selected_user && users.some((user) => user.id === detection.selected_user)) {
    return detection.selected_user
  }
  return users.length === 1 ? users[0].id : ''
}

export function canPlan(source: string, user: string, users: HemoryUser[], needsTimezone: boolean, timezone: string): boolean {
  return Boolean(
    source
      && (users.length <= 1 || user)
      && (!needsTimezone || timezone.trim()),
  )
}

export function hasProblems(report: MigrationReport): boolean {
  return report.conflict > 0 || report.blocked > 0 || report.errors.length > 0
}

export const REPORT_ACTIONS: MigrationAction[] = [
  'create',
  'update',
  'skip',
  'conflict',
  'blocked',
  'source_missing',
  'excluded',
]

export function reportCount(report: MigrationReport, action: MigrationAction): number {
  if (action === 'excluded') return report.excluded_audio
  return report[action]
}

export function actionTone(action: MigrationAction): 'positive' | 'warning' | 'danger' | 'neutral' {
  if (action === 'create' || action === 'update') return 'positive'
  if (action === 'conflict' || action === 'blocked') return 'danger'
  if (action === 'source_missing') return 'warning'
  return 'neutral'
}

export function progressPercent(committed: number, total: number): number {
  if (total <= 0) return 0
  return Math.max(0, Math.min(100, Math.round((committed / total) * 100)))
}
