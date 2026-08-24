import { describe, it, expect, vi, beforeEach } from 'vitest'

// The whole point of `openPath` is the file-vs-directory fork, so `stat` is the
// only interesting input; every collaborator is mocked to record the fork taken.
const statMock = vi.fn(async (..._a: unknown[]) => ({ isDirectory: false }))
vi.mock('@tauri-apps/plugin-fs', () => ({
  stat: (...a: unknown[]) => statMock(...a),
}))

const openFileMock = vi.fn(async (..._a: unknown[]) => {})
vi.mock('./tabs.svelte', () => ({
  openFile: (...a: unknown[]) => openFileMock(...a),
}))

const setRootDirMock = vi.fn(async (..._a: unknown[]) => {})
vi.mock('./folder-view.svelte', () => ({
  setRootDir: (...a: unknown[]) => setRootDirMock(...a),
}))

const setActiveViewMock = vi.fn(async (..._a: unknown[]) => {})
const setSideVisibleMock = vi.fn(async (..._a: unknown[]) => {})
vi.mock('./side-panel/registry.svelte', () => ({
  setActiveView: (...a: unknown[]) => setActiveViewMock(...a),
  setSideVisible: (...a: unknown[]) => setSideVisibleMock(...a),
}))

import { openPath } from './open-path'

beforeEach(() => {
  statMock.mockReset(); statMock.mockResolvedValue({ isDirectory: false })
  openFileMock.mockReset()
  setRootDirMock.mockReset()
  setActiveViewMock.mockReset()
  setSideVisibleMock.mockReset()
})

describe('openPath', () => {
  it('opens a file as a tab', async () => {
    await openPath('/vault/xxx.md')
    expect(openFileMock).toHaveBeenCalledWith('/vault/xxx.md')
    expect(setRootDirMock).not.toHaveBeenCalled()
  })

  it('makes a directory the folder view root and reveals the panel', async () => {
    // `notemd .` — the CLI resolves the cwd to an absolute path first.
    statMock.mockResolvedValue({ isDirectory: true })
    await openPath('/vault/notes')
    expect(setRootDirMock).toHaveBeenCalledWith('/vault/notes')
    expect(setActiveViewMock).toHaveBeenCalledWith('left', 'folder-view')
    expect(setSideVisibleMock).toHaveBeenCalledWith('left', true)
    // A directory is not a document: no tab is opened for it.
    expect(openFileMock).not.toHaveBeenCalled()
  })

  it('falls back to openFile when stat fails', async () => {
    // A missing path must produce openFile's existing "cannot read" error, not
    // a second error message invented here.
    statMock.mockRejectedValue(new Error('ENOENT'))
    await openPath('/gone.md')
    expect(openFileMock).toHaveBeenCalledWith('/gone.md')
  })
})
