import type {
  HemoryDetection,
  MeetingSummary,
  MigrationMode,
  MigrationReport,
} from './types'

export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window {
    notemd: NotemdBridge
  }
}

export function bridge(): NotemdBridge {
  const value = window.notemd
  if (!value) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return value
}

export async function vaultInfo(): Promise<{ root: string | null }> {
  return bridge().request('host.vault.info', {})
}

export async function pickHemoryDirectory(title: string): Promise<string | null> {
  const result: { paths: string[] | null } = await bridge().request('host.dialog.open', {
    title,
    directory: true,
    multiple: false,
  })
  return result.paths?.[0] ?? null
}

export async function listMeetings(): Promise<MeetingSummary[]> {
  const result = await bridge().request('plugin.library_list', {})
  return Array.isArray(result?.meetings) ? result.meetings : []
}

export function detectHemory(source: string): Promise<HemoryDetection> {
  return bridge().request('plugin.hemory_detect', { source })
}

export function planHemory(options: {
  source: string
  user?: string
  timezone?: string
  mode: MigrationMode
}): Promise<MigrationReport> {
  return bridge().request('plugin.hemory_plan', {
    source: options.source,
    mode: options.mode,
    ...(options.user ? { user: options.user } : {}),
    ...(options.timezone ? { timezone: options.timezone } : {}),
  })
}

export function startHemoryMigration(options: {
  source: string
  user?: string
  timezone?: string
  mode: MigrationMode
  expected_plan: MigrationReport
}): Promise<{ job_id: number }> {
  return bridge().request('plugin.hemory_apply_start', {
    source: options.source,
    mode: options.mode,
    expected_plan: options.expected_plan,
    ...(options.user ? { user: options.user } : {}),
    ...(options.timezone ? { timezone: options.timezone } : {}),
  })
}

export function cancelHemoryMigration(jobId: number): Promise<void> {
  return bridge().request('plugin.hemory_cancel', { job_id: jobId })
}

export function openInEditor(path: string): Promise<void> {
  return bridge().request('host.editor.open', { path })
}

export async function toast(
  level: 'success' | 'info' | 'warn' | 'error',
  message: string,
  detail?: string,
): Promise<void> {
  try {
    await bridge().request('host.toast', { level, message, detail })
  } catch {
    // A toast is helpful feedback, never part of the migration transaction.
  }
}
