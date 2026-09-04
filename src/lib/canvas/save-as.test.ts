import { beforeEach, describe, expect, it, vi } from 'vitest'
import { confirmCanvasSaveAsReferences } from './save-as'

const ask = vi.hoisted(() => vi.fn(async () => true))
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask }))
vi.mock('../sotvault.svelte', () => ({ sotvaultStore: { vaultRoot: null } }))
vi.mock('../folder-view.svelte', () => ({ folderView: { rootDir: null } }))

const canvasWithReference = JSON.stringify({
  nodes: [{ id: 'f', type: 'file', file: 'asset.png', x: 0, y: 0, width: 100, height: 100 }],
  edges: [],
})

describe('Canvas Save As reference policy', () => {
  beforeEach(() => ask.mockClear())

  it('does not prompt while the resource root stays the same', async () => {
    await expect(confirmCanvasSaveAsReferences({
      filePath: '/vault/board.canvas', currentContent: canvasWithReference,
    }, '/vault/copy.canvas')).resolves.toBe(true)
    expect(ask).not.toHaveBeenCalled()
  })

  it('requires acknowledgement before a cross-root copy with references', async () => {
    ask.mockResolvedValueOnce(false)
    await expect(confirmCanvasSaveAsReferences({
      filePath: '/vault/board.canvas', currentContent: canvasWithReference,
    }, '/other/copy.canvas')).resolves.toBe(false)
    expect(ask).toHaveBeenCalledWith(expect.stringContaining('1 个'), expect.objectContaining({ kind: 'warning' }))
  })

  it('does not prompt for a reference-free document', async () => {
    await expect(confirmCanvasSaveAsReferences({
      filePath: '/vault/board.canvas', currentContent: '{"nodes":[],"edges":[]}',
    }, '/other/copy.canvas')).resolves.toBe(true)
    expect(ask).not.toHaveBeenCalled()
  })
})
