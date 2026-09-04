/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const canvasRevisionA = { mtimeNs: '1000000000', size: 24, sha256: 'canvas-a' }
const canvasRevisionB = { mtimeNs: '2000000000', size: 25, sha256: 'canvas-b' }
const canvasOpen = vi.fn()

vi.mock('./fs', () => ({
  readMd: vi.fn(),
  writeMd: vi.fn(),
  basename: (p: string) => p.split('/').pop() ?? p,
  classifyPath: (path: string) => path.endsWith('.canvas') ? { kind: 'canvas' } : { kind: 'markdown' },
  isSupportedPath: () => true,
  looksBinary: () => false,
  modeKeyFor: () => 'md',
  statFile: vi.fn(),
}))

vi.mock('./canvas/io', () => ({
  canvasDocumentOpen: (path: string) => canvasOpen(path),
  canvasMtimeMs: (revision: { mtimeNs: string }) => Number(BigInt(revision.mtimeNs) / 1_000_000n),
  asCanvasDocumentError: (error: unknown) => error && typeof error === 'object' ? error : null,
}))

vi.mock('./settings.svelte', () => ({
  pushRecentFile: vi.fn(async () => {}),
  getRecentMode: vi.fn(() => null),
  setRecentMode: vi.fn(async () => {}),
  settings: { autoSave: false },
}))

vi.mock('@tauri-apps/plugin-fs', () => ({
  watchImmediate: vi.fn(async () => () => {}),
}))

vi.mock('./platform.svelte', () => ({ isIOS: vi.fn(async () => false) }))

beforeEach(async () => {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  vi.clearAllMocks()
  vi.resetModules()
})

describe('verifyAllOpen', () => {
  it('auto-reloads a clean Canvas from one bounded snapshot and adopts its exact revision', async () => {
    canvasOpen
      .mockResolvedValueOnce({
        text: '{"nodes":[],"edges":[]}', revision: canvasRevisionA,
        requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas',
      })
      .mockResolvedValueOnce({
        text: '{"nodes":[],"edges":[],"next":true}', revision: canvasRevisionB,
        requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas',
      })
    const fs = await import('./fs')
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/board.canvas')

    await watcher.verifyAllOpen()

    const tab = tabs.tabs[0]
    expect(tab.currentContent).toContain('"next":true')
    expect(tab.initialContent).toBe(tab.currentContent)
    expect(tab.canvasRevision).toEqual(canvasRevisionB)
    expect(tab.lastKnownHash).toBe('canvas-b')
    expect(tab.lastKnownMtime).toBe(2000)
    expect(fs.statFile).not.toHaveBeenCalled()
    expect(fs.readMd).not.toHaveBeenCalled()
  })

  it('refreshes the exact Canvas revision after a same-content touch', async () => {
    const touched = { ...canvasRevisionA, mtimeNs: '3000000000' }
    const content = '{"nodes":[],"edges":[]}'
    canvasOpen
      .mockResolvedValueOnce({ text: content, revision: canvasRevisionA, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
      .mockResolvedValueOnce({ text: content, revision: touched, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
    const fs = await import('./fs')
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/board.canvas')

    await watcher.verifyAllOpen()

    expect(tabs.tabs[0].canvasRevision).toEqual(touched)
    expect(tabs.tabs[0].lastKnownMtime).toBe(3000)
    expect(fs.statFile).not.toHaveBeenCalled()
    expect(fs.readMd).not.toHaveBeenCalled()
  })

  it('keeps a dirty Canvas buffer and stages the bounded external snapshot', async () => {
    canvasOpen
      .mockResolvedValueOnce({ text: '{"nodes":[],"edges":[]}', revision: canvasRevisionA, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
      .mockResolvedValueOnce({ text: '{"nodes":[],"edges":[1]}', revision: canvasRevisionB, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
    const fs = await import('./fs')
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/board.canvas')
    tabs.setContent(tabs.tabs[0].id, '{"nodes":[1],"edges":[]}')

    await watcher.verifyAllOpen()

    const tab = tabs.tabs[0]
    expect(tab.currentContent).toContain('"nodes":[1]')
    expect(tab.canvasRevision).toEqual(canvasRevisionA)
    expect(tab.externalState).toBe('changed')
    expect(tab.pendingExternal).toMatchObject({ hash: 'canvas-b', content: '{"nodes":[],"edges":[1]}' })
    expect(fs.statFile).not.toHaveBeenCalled()
    expect(fs.readMd).not.toHaveBeenCalled()
  })

  it('never falls back to generic reads when a Canvas exceeds the bounded snapshot limit', async () => {
    canvasOpen
      .mockResolvedValueOnce({ text: '{"nodes":[],"edges":[]}', revision: canvasRevisionA, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
      .mockRejectedValueOnce({ kind: 'tooLarge', message: 'too large', limitBytes: 32, actualBytes: 33 })
    const fs = await import('./fs')
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/board.canvas')

    await watcher.verifyAllOpen()

    expect(tabs.tabs[0].externalState).toBe('changed')
    expect(tabs.tabs[0].pendingExternal).toBeUndefined()
    expect(fs.statFile).not.toHaveBeenCalled()
    expect(fs.readMd).not.toHaveBeenCalled()
  })

  it('stages invalid external Canvas bytes instead of silently replacing a clean baseline', async () => {
    canvasOpen
      .mockResolvedValueOnce({ text: '{"nodes":[],"edges":[]}', revision: canvasRevisionA, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
      .mockResolvedValueOnce({ text: '{"nodes":[', revision: canvasRevisionB, requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas' })
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/board.canvas')

    await watcher.verifyAllOpen()

    expect(tabs.tabs[0].currentContent).toBe('{"nodes":[],"edges":[]}')
    expect(tabs.tabs[0].externalState).toBe('changed')
    expect(tabs.tabs[0].pendingExternal?.content).toBe('{"nodes":[')
    expect(tabs.tabs[0].canvasRevision).toEqual(canvasRevisionA)
  })

  it('marks a clean tab autoReload when disk content differs', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')          // initial open
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })  // open
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('B')          // verify pass
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 2000, size: 1 })  // verify
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/foo.md')
    tabs.toggleMode(tabs.tabs[0].id)              // → source: only source-mode tabs autoReload
    await watcher.verifyAllOpen()
    const t = tabs.tabs[0]
    expect(t.externalState).toBe('fresh')         // clean source tab → auto-reloaded, stays fresh
    expect(t.initialContent).toBe('B')
    expect(t.currentContent).toBe('B')
    expect(t.lastKnownMtime).toBe(2000)
  })

  it('rich-mode clean tab + external modify → changed (banner), no silent reload', async () => {
    // Regression: previously rich-mode tabs hit the autoReload fast-path,
    // which left the ProseMirror view stale while tab.currentContent was
    // swapped underneath. The next keystroke/destroy-flush then overwrote
    // the disk's new content with the editor's pre-change state.
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('B')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 2000, size: 1 })
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/foo.md')
    tabs.setMode(tabs.tabs[0].id, 'rich')
    await watcher.verifyAllOpen()
    const t = tabs.tabs[0]
    expect(t.externalState).toBe('changed')
    expect(t.pendingExternal?.content).toBe('B')
    // Critical: tab.currentContent must NOT have been silently swapped.
    expect(t.currentContent).toBe('A')
    expect(t.initialContent).toBe('A')
  })

  it('marks a dirty tab as changed when disk content differs', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('B')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 2000, size: 1 })
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/foo.md')
    tabs.setContent(tabs.tabs[0].id, 'edited')
    await watcher.verifyAllOpen()
    const t = tabs.tabs[0]
    expect(t.externalState).toBe('changed')
    expect(t.pendingExternal?.content).toBe('B')
    expect(t.pendingExternal?.mtime).toBe(2000)
  })

  it('marks a tab as deleted when stat returns null', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(null)        // deleted
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/foo.md')
    await watcher.verifyAllOpen()
    expect(tabs.tabs[0].externalState).toBe('deleted')
  })

  it('does nothing when stat returns the same mtime and content (no-op poll)', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    await tabs.openFile('/tmp/foo.md')
    await watcher.verifyAllOpen()
    expect(tabs.tabs[0].externalState).toBe('fresh')
  })
})

describe('startWatchingTab / stopWatchingTab', () => {
  it('startWatchingTab subscribes via watchImmediate and stop unsubscribes', async () => {
    const unwatch = vi.fn()
    const plug = await import('@tauri-apps/plugin-fs')
    ;(plug.watchImmediate as ReturnType<typeof vi.fn>).mockResolvedValueOnce(unwatch)
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    await tabs.openFile('/tmp/foo.md')
    await watcher.startWatchingTab(tabs.tabs[0])
    expect(plug.watchImmediate).toHaveBeenCalledWith('/tmp/foo.md', expect.any(Function))
    await watcher.stopWatchingTab(tabs.tabs[0].id)
    expect(unwatch).toHaveBeenCalled()
  })

  it('rebindTabPath stops the old subscription and starts a new one', async () => {
    const unwatchOld = vi.fn()
    const unwatchNew = vi.fn()
    const plug = await import('@tauri-apps/plugin-fs')
    ;(plug.watchImmediate as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(unwatchOld)
      .mockResolvedValueOnce(unwatchNew)
    const tabs = await import('./tabs.svelte')
    const watcher = await import('./file-watcher.svelte')
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('A')
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({ mtime: 1000, size: 1 })
    await tabs.openFile('/tmp/foo.md')
    await watcher.startWatchingTab(tabs.tabs[0])
    // Caller owns filePath: set it first, then rebind.
    tabs.tabs[0].filePath = '/tmp/bar.md'
    await watcher.rebindTabPath(tabs.tabs[0].id)
    expect(unwatchOld).toHaveBeenCalled()
    expect(plug.watchImmediate).toHaveBeenLastCalledWith('/tmp/bar.md', expect.any(Function))
  })
})

describe('installFocusPoll', () => {
  it('attaches a window focus listener that calls verifyAllOpen', async () => {
    const watcher = await import('./file-watcher.svelte')
    const spy = vi.spyOn(watcher, 'verifyAllOpen')
      .mockImplementation(async () => {})
    const uninstall = watcher.installFocusPoll()
    window.dispatchEvent(new Event('focus'))
    expect(spy).toHaveBeenCalledTimes(1)
    uninstall()
    window.dispatchEvent(new Event('focus'))
    expect(spy).toHaveBeenCalledTimes(1)  // not called after uninstall
  })
})
