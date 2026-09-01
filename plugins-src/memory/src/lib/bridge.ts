import type { DecideInput, Proposal, ProposeInput, Snapshot } from './types'

export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global { interface Window { notemd: NotemdBridge } }

export function bridge(): NotemdBridge {
  if (!window.notemd) throw new Error('window.notemd bridge missing')
  return window.notemd
}

export function memoryList(): Promise<Snapshot> { return bridge().request('host.memory.list', {}) }
export function memorySuggest(): Promise<{ suggestions: unknown[] }> { return bridge().request('host.memory.suggest', {}) }
export function memoryPropose(input: ProposeInput): Promise<Proposal> { return bridge().request('host.memory.propose', input) }
export function memoryDecide(input: DecideInput): Promise<unknown> { return bridge().request('host.memory.decide', input) }
export function memoryMigrate(): Promise<{ migrated: number; already_managed: boolean }> { return bridge().request('host.memory.migrate', {}) }

export async function toast(level: 'success' | 'info' | 'warn' | 'error', message: string, detail?: string): Promise<void> {
  try { await bridge().request('host.toast', { level, message, detail }) } catch { /* non-critical */ }
}
