import { beforeEach, describe, expect, it, vi } from 'vitest'

const tab = {
  id: 'canvas-tab',
  kind: 'canvas',
  filePath: '/tmp/board.canvas',
  currentContent: '{"nodes":[],"edges":[]}',
}
const saveAs = vi.fn(async () => {})
const exportCanvasCopy = vi.fn(async () => {})
const isIOS = vi.fn(async () => false)
const pickSaveCanvasFile = vi.fn(async () => '/tmp/copy.canvas')
const confirmCanvasSaveAsReferences = vi.fn(async () => true)
const toggleSideView = vi.fn(async () => {})

vi.mock('./tabs.svelte', () => ({
  activeTab: () => tab,
  saveActive: vi.fn(),
  saveAs,
  exportCanvasCopy,
  openFile: vi.fn(),
  closeTab: vi.fn(),
  toggleMode: vi.fn(),
  newCanvas: vi.fn(),
}))

vi.mock('./dialogs', () => ({
  confirmDirtyClose: vi.fn(),
  pickOpenFile: vi.fn(),
  pickSaveCanvasFile,
  pickSaveFile: vi.fn(),
  showError: vi.fn(),
}))

vi.mock('./platform.svelte', () => ({ isIOS }))
vi.mock('./canvas/save-as', () => ({ confirmCanvasSaveAsReferences }))
vi.mock('./share', () => ({
  sharePublishCurrent: vi.fn(), shareUnpublishCurrent: vi.fn(), shareCopyLinkCurrent: vi.fn(),
}))
vi.mock('./print', () => ({ printActiveTab: vi.fn() }))
vi.mock('./sotvault.svelte', () => ({
  syncCurrentToVault: vi.fn(), deviceSourceForVaultPath: vi.fn(), revealVaultSource: vi.fn(),
}))
vi.mock('./side-panel/registry.svelte', () => ({ toggleSideView }))
vi.mock('./ui-state.svelte', () => ({ openSettings: vi.fn() }))

beforeEach(() => {
  vi.clearAllMocks()
  isIOS.mockResolvedValue(false)
  pickSaveCanvasFile.mockResolvedValue('/tmp/copy.canvas')
  confirmCanvasSaveAsReferences.mockResolvedValue(true)
})

describe('Canvas Save As command', () => {
  it('exports a copy on iOS without rebinding the current tab', async () => {
    isIOS.mockResolvedValueOnce(true)
    const { cmdSaveAs } = await import('./commands')

    await cmdSaveAs()

    expect(exportCanvasCopy).toHaveBeenCalledWith('canvas-tab', '/tmp/copy.canvas')
    expect(saveAs).not.toHaveBeenCalled()
  })

  it('keeps identity-changing Save As on desktop', async () => {
    const { cmdSaveAs } = await import('./commands')

    await cmdSaveAs()

    expect(saveAs).toHaveBeenCalledWith('canvas-tab', '/tmp/copy.canvas')
    expect(exportCanvasCopy).not.toHaveBeenCalled()
  })
})

describe('Table of contents command', () => {
  it('opens the registered table-of-contents side view', async () => {
    const { dispatch } = await import('./commands')
    await dispatch('toggle-table-of-contents')
    expect(toggleSideView).toHaveBeenCalledWith('table-of-contents')
  })
})
