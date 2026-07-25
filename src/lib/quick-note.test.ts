import { describe, it, expect, vi, beforeEach } from 'vitest'

const invoke = vi.fn()
const mkdir = vi.fn()
const exists = vi.fn()
const openFile = vi.fn()
const openPathBackedMarkdownDraft = vi.fn()
const requestEditorFocus = vi.fn()
const pushToast = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))
vi.mock('@tauri-apps/plugin-fs', () => ({
  mkdir: (...a: unknown[]) => mkdir(...a),
  exists: (...a: unknown[]) => exists(...a),
}))
vi.mock('./tabs.svelte', () => ({
  openFile: (...a: unknown[]) => openFile(...a),
  openPathBackedMarkdownDraft: (...a: unknown[]) => openPathBackedMarkdownDraft(...a),
}))
vi.mock('./editor-focus.svelte', () => ({
  requestEditorFocus: (...a: unknown[]) => requestEditorFocus(...a),
}))
vi.mock('./toast.svelte', () => ({
  pushToast: (...a: unknown[]) => pushToast(...a),
}))
vi.mock('./i18n/store.svelte', () => ({
  t: (k: string) => k,
}))

beforeEach(() => {
  vi.clearAllMocks()
  invoke.mockResolvedValue('/vault/inbox')
  mkdir.mockResolvedValue(undefined)
  exists.mockResolvedValue(false)
  openFile.mockResolvedValue(undefined)
  openPathBackedMarkdownDraft.mockResolvedValue(undefined)
})

describe('createQuickNote', () => {
  it('opens a lazy path-backed draft when the target file does not exist', async () => {
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    const path = '/vault/inbox/2026-07-25-09-08-Quick.md'
    expect(invoke).toHaveBeenCalledWith('notemd_quick_note_dir')
    expect(mkdir).toHaveBeenCalledWith('/vault/inbox', { recursive: true })
    expect(requestEditorFocus).toHaveBeenCalledWith(path)
    expect(openPathBackedMarkdownDraft).toHaveBeenCalledWith(path, '', {
      mode: 'rich',
      skipEmptySave: true,
    })
    expect(openFile).not.toHaveBeenCalled()
  })

  it('opens an existing same-minute quick note normally', async () => {
    exists.mockResolvedValue(true)
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    const path = '/vault/inbox/2026-07-25-09-08-Quick.md'
    expect(openFile).toHaveBeenCalledWith(path)
    expect(openPathBackedMarkdownDraft).not.toHaveBeenCalled()
  })

  it('shows the no-vault toast when the backend has no quick-note dir', async () => {
    invoke.mockRejectedValue(new Error('Vault not configured'))
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    expect(pushToast).toHaveBeenCalledWith({
      level: 'warn',
      message: 'quickNote.noVault',
    })
    expect(mkdir).not.toHaveBeenCalled()
  })
})
