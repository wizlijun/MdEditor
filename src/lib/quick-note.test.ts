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

// Default: identity cache is cold — createQuickNote() must sign nothing until
// a test opts into a warm cache.
const humanActorNow = vi.fn((): string | null => null)
vi.mock('./okf/identity', () => ({
  humanActorNow: () => humanActorNow(),
}))

beforeEach(() => {
  vi.clearAllMocks()
  invoke.mockResolvedValue('/vault/inbox')
  mkdir.mockResolvedValue(undefined)
  exists.mockResolvedValue(false)
  openFile.mockResolvedValue(undefined)
  openPathBackedMarkdownDraft.mockResolvedValue(undefined)
  humanActorNow.mockReturnValue(null)
})

describe('createQuickNote', () => {
  it('opens a lazy path-backed draft when the target file does not exist', async () => {
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    const path = '/vault/inbox/2026-07-25-090800-quick.md'
    expect(invoke).toHaveBeenCalledWith('notemd_quick_note_dir')
    expect(mkdir).toHaveBeenCalledWith('/vault/inbox', { recursive: true })
    expect(requestEditorFocus).toHaveBeenCalledWith(path)
    // No `mode`: the draft inherits the editor's remembered mode for `.md`.
    // 草稿预置 OKF 概念头(§4.1 必填 type),这样保存下来的就是合规文档;
    // 光标落在文末(Selection.atEnd),不会掉进 frontmatter 里。
    expect(openPathBackedMarkdownDraft).toHaveBeenCalledWith(path, '---\ntype: Note\n---\n', {
      skipEmptySave: true,
    })
    expect(openFile).not.toHaveBeenCalled()
  })

  it('signs the draft via humanActorNow() when the identity cache is warm', async () => {
    // Wiring test: drives the real createQuickNote() call site with a warm
    // identity cache and asserts the signature reached the text handed to
    // the draft opener — catches a renamed { by, at } shape or a dropped
    // conditional that a hand-built-author unit test on newFileText cannot.
    humanActorNow.mockReturnValue('human:testuser')
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    const path = '/vault/inbox/2026-07-25-090800-quick.md'
    expect(openPathBackedMarkdownDraft).toHaveBeenCalledWith(
      path,
      expect.stringContaining('generated:\n  by: human:testuser\n  at:'),
      { skipEmptySave: true },
    )
  })

  it('opens an existing same-minute quick note normally', async () => {
    exists.mockResolvedValue(true)
    const { createQuickNote } = await import('./quick-note.svelte')
    await createQuickNote(new Date(2026, 6, 25, 9, 8))

    const path = '/vault/inbox/2026-07-25-090800-quick.md'
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
