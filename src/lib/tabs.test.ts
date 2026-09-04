import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { CanvasProbeResult } from './canvas/io'

vi.mock('./fs', () => ({
  readMd: vi.fn(async (p: string) => `# content of ${p}`),
  writeMd: vi.fn(async () => {}),
  basename: (p: string) => p.split('/').pop() ?? p,
  classifyPath: (p: string) => {
    const lower = p.toLowerCase()
    if (/\.(md|markdown|mdown|mkd)$/.test(lower)) return { kind: 'markdown' }
    if (/\.html?$/.test(lower)) return { kind: 'html' }
    if (/\.py$/.test(lower)) return { kind: 'code', language: 'python' }
    if (/\.json$/.test(lower)) return { kind: 'code', language: 'json' }
    if (/\.txt$/.test(lower)) return { kind: 'code', language: '' }
    if (/\.csv$/.test(lower)) return { kind: 'spreadsheet' }
    if (/\.canvas$/.test(lower)) return { kind: 'canvas' }
    if (/\.tsv$/.test(lower)) return { kind: 'code', language: '' }
    if (/\.(png|jpg|jpeg|gif|webp|svg|bmp|heic|heif|avif)$/.test(lower)) return { kind: 'image' }
    return null
  },
  isSupportedPath: (p: string) => /\.(md|markdown|mdown|mkd|html?|py|json|txt|csv|tsv|canvas|png|jpg|jpeg|gif|webp|svg|bmp|heic|heif|avif)$/i.test(p),
  looksBinary: (s: string) => s.indexOf('\x00') >= 0,
  modeKeyFor: (p: string) => {
    const base = (p.split('/').pop() ?? p).toLowerCase()
    const dot = base.lastIndexOf('.')
    return dot <= 0 ? base : base.slice(dot + 1)
  },
  statFile: vi.fn(async () => ({ mtime: 1_700_000_000_000, size: 100 })),
}))

vi.mock('./settings.svelte', () => ({
  pushRecentFile: vi.fn(async () => {}),
  getRecentMode: vi.fn(() => null),
  setRecentMode: vi.fn(async () => {}),
  settings: { autoSave: false },
}))

vi.mock('./file-watcher.svelte', () => ({
  startWatchingTab: vi.fn(async () => {}),
  stopWatchingTab: vi.fn(async () => {}),
  rebindTabPath: vi.fn(async () => {}),
  verifyAllOpen: vi.fn(async () => {}),
}))

// Default: pickSaveFile returns a path (simulates user completing the save panel)
vi.mock('./dialogs', () => ({
  pickSaveFile: vi.fn(async (defaultPath?: string) => defaultPath ?? '/tmp/untitled.md'),
  pickSaveCanvasFile: vi.fn(async (defaultPath?: string) => defaultPath ?? '/tmp/untitled.canvas'),
  confirmDirtyClose: vi.fn(async () => 'discard'),
  pickOpenFile: vi.fn(async () => null),
  showError: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  ask: vi.fn(async () => false),  // default: user clicks "Keep Editing"
}))

const canvasRevision = { mtimeNs: '1700000000000000000', size: 27, sha256: 'canvas-hash' }
const canvasOpen = vi.fn(async (path: string) => ({
  text: '{"nodes":[],"edges":[]}\n', revision: canvasRevision,
  requestedPath: path, canonicalPath: path,
}))
const canvasCreate = vi.fn(async (path: string, _text: string) => ({ revision: canvasRevision, canonicalPath: path }))
const canvasProbe = vi.fn<(path: string) => Promise<CanvasProbeResult>>(async (path: string) => ({
  kind: 'present' as const, revision: canvasRevision, requestedPath: path, canonicalPath: path,
}))
const canvasSave = vi.fn(async (
  path: string,
  _text: string,
  _revision?: typeof canvasRevision,
  _force?: boolean,
) => ({ revision: canvasRevision, canonicalPath: path }))
vi.mock('./canvas/io', () => ({
  canvasDocumentOpen: (path: string) => canvasOpen(path),
  canvasDocumentProbe: (path: string) => canvasProbe(path),
  canvasDocumentCreate: (path: string, text: string) => canvasCreate(path, text),
  canvasDocumentSave: (path: string, text: string, revision: typeof canvasRevision, force?: boolean) =>
    canvasSave(path, text, revision, force),
  canvasMtimeMs: () => 1_700_000_000_000,
  asCanvasDocumentError: (error: unknown) => error && typeof error === 'object' ? error : null,
}))

vi.mock('./i18n/store.svelte', () => ({
  t: (k: string) => k,
}))

vi.mock('./platform.svelte', () => ({ isIOS: vi.fn(async () => false) }))

const fsRename = vi.fn(async (_from: string, _to: string) => {})
const fsExists = vi.fn(async (_path: string) => false)
vi.mock('@tauri-apps/plugin-fs', () => ({
  rename: (from: string, to: string) => fsRename(from, to),
  exists: (path: string) => fsExists(path),
}))

vi.mock('@tauri-apps/plugin-store', () => ({
  Store: {
    load: vi.fn(async () => ({
      get: vi.fn(async () => null),
      set: vi.fn(async () => {}),
      delete: vi.fn(async () => false),
      save: vi.fn(async () => {}),
    })),
  },
}))

// Default: identity cache is cold (matches app boot before warmHumanActor()
// resolves) — newFile() must sign nothing until a test opts into a warm cache.
const humanActorNowMock = vi.fn((): string | null => null)
vi.mock('./okf/identity', () => ({
  humanActorNow: () => humanActorNowMock(),
}))

beforeEach(async () => {
  // Let dynamic imports started by the preceding save/watcher operation
  // finish before invalidating the module graph.
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  vi.clearAllMocks()
  vi.resetModules()
  fsExists.mockResolvedValue(false)
  humanActorNowMock.mockReturnValue(null)
})

describe('tabs', () => {
  it('openFile reads file and creates a tab', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    expect(m.tabs.length).toBe(1)
    expect(m.tabs[0].filePath).toBe('/tmp/foo.md')
    expect(m.tabs[0].title).toBe('foo.md')
    expect(m.tabs[0].currentContent).toContain('content of /tmp/foo.md')
    expect(m.activeId.value).toBe(m.tabs[0].id)
  })

  it('openFile is idempotent: same path → switch tab, no duplicate', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/a.md')
    await m.openFile('/tmp/b.md')
    expect(m.tabs.length).toBe(2)
    expect(m.activeId.value).toBe(m.tabs[1].id)
    await m.openFile('/tmp/a.md')
    expect(m.tabs.length).toBe(2)
    expect(m.activeId.value).toBe(m.tabs[0].id)
  })

  it('keeps a controlled memory projection read-only across edit and save paths', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('# MEMORY\n')
    const projection = await import('./memory-projection')
    projection.setMemoryProjectionVaultRoot('/vault')
    const m = await import('./tabs.svelte')
    await m.openFile('/vault/MEMORY.md')
    const tab = m.tabs[0]
    const original = tab.currentContent

    expect(m.isManagedMemoryTab(tab)).toBe(true)
    m.setContent(tab.id, '# direct edit')
    expect(tab.currentContent).toBe(original)

    // A second guard at persistence time covers any component that mutates the
    // tab object without going through setContent.
    tab.currentContent = '# bypassed UI guard'
    await m.saveActive()
    await m.saveTab(tab.id)
    await m.overwriteOnDisk(tab.id)
    await m.restoreVersion(tab.id, '# old version')
    await m.saveAs(tab.id, '/tmp/copy.md')

    expect(fs.writeMd).not.toHaveBeenCalled()
    expect(tab.filePath).toBe('/vault/MEMORY.md')
    expect(tab.initialContent).toBe(original)
  })

  it('openFile falls back to plain text (kind=code) for an unknown extension with no plugin', async () => {
    // file-over-app: an unrecognised extension (and no custom-editor plugin
    // claiming it) opens as plain text instead of throwing.
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/notes.base')
    expect(m.tabs.length).toBe(1)
    const t = m.tabs[0]
    expect(t.kind).toBe('code')
    expect(t.language).toBe('')
    expect(t.currentContent).toContain('content of /tmp/notes.base')
    expect(m.activeId.value).toBe(t.id)
  })

  it('openFile plain-text fallback still refuses binary content', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('bin\x00ary')
    const m = await import('./tabs.svelte')
    await expect(m.openFile('/tmp/foo.weird')).rejects.toThrow(/binary/i)
    expect(m.tabs.length).toBe(0)
  })

  it('openFile plain-text fallback is idempotent (same path → switch, no dup)', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/a.unknownext')
    await m.openFile('/tmp/a.unknownext')
    expect(m.tabs.length).toBe(1)
  })

  it('openFile routes to a custom editor when a v2 plugin claims the extension', async () => {
    // Register a plugin (in the same fresh module graph) that owns .base, then
    // open a .base → the tab becomes kind=custom carrying the editor binding.
    const rt = await import('./plugins/runtime.svelte')
    rt.pluginRuntime.manifests = [{
      id: 'notemd.base', name: 'Base', version: '1.0.0', binary: '',
      host_capabilities: [],
      custom_editors: [{ id: 'base-table', file_extensions: ['.base'], entry: 'editor.html' }],
    }]
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/table.base')
    const t = m.tabs[0]
    expect(t.kind).toBe('custom')
    expect(t.editorPluginId).toBe('notemd.base')
    expect(t.editorId).toBe('base-table')
    expect(t.editorEntry).toBe('editor.html')
    // Content is still read as text (host owns document I/O).
    expect(t.currentContent).toContain('content of /tmp/table.base')
  })

  it('openFile keeps .canvas on the built-in surface when a plugin claims it', async () => {
    const rt = await import('./plugins/runtime.svelte')
    rt.pluginRuntime.manifests = [{
      id: 'canvas.hijacker', name: 'Canvas Hijacker', version: '1.0.0', binary: '',
      host_capabilities: [],
      custom_editors: [{ id: 'canvas-editor', file_extensions: ['.canvas'], entry: 'editor.html' }],
    }]
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    expect(m.tabs[0].kind).toBe('canvas')
    expect(m.tabs[0].mode).toBe('rich')
    expect(m.tabs[0].editorId).toBeUndefined()
  })

  it('canvas mode is fixed to rich and never writes a recent editor mode', async () => {
    const settings = await import('./settings.svelte')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const tab = m.tabs[0]
    m.setMode(tab.id, 'source')
    m.toggleMode(tab.id)
    expect(tab.mode).toBe('rich')
    expect(settings.setRecentMode).not.toHaveBeenCalled()
    await m.saveActive()
    expect(settings.setRecentMode).not.toHaveBeenCalled()
  })

  it('saveAs keeps the canvas extension boundary in both directions', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    await expect(m.saveAs(canvas.id, '/tmp/board.json')).rejects.toThrow(/\.canvas/i)
    expect(fs.writeMd).not.toHaveBeenCalled()

    await m.openFile('/tmp/note.md')
    const markdown = m.tabs[1]
    await expect(m.saveAs(markdown.id, '/tmp/note.canvas')).rejects.toThrow(/only canvas/i)
    expect(fs.writeMd).not.toHaveBeenCalled()
  })

  it('saves canvas through the revision-checked writer and marks that snapshot clean', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    const next = '{"nodes":[{"id":"n","type":"text","text":"one","x":0,"y":0,"width":100,"height":100}],"edges":[]}'
    m.setContent(canvas.id, next)

    await m.saveActive()

    expect(canvasSave).toHaveBeenCalledWith('/tmp/board.canvas', next, canvasRevision, false)
    expect(canvas.initialContent).toBe(next)
    expect(canvas.currentContent).toBe(next)
    expect(canvas.externalState).toBe('fresh')
  })

  it('does not mark a newer canvas edit clean when an older save finishes later', async () => {
    let finishSave: ((value: { revision: typeof canvasRevision; canonicalPath: string }) => void) | undefined
    canvasSave.mockImplementationOnce(() => new Promise((resolve) => { finishSave = resolve }))
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    const savedSnapshot = '{"nodes":[],"edges":[],"snapshot":1}'
    const newerSnapshot = '{"nodes":[],"edges":[],"snapshot":2}'
    m.setContent(canvas.id, savedSnapshot)

    const saving = m.saveActive()
    await vi.waitFor(() => expect(canvasSave).toHaveBeenCalledTimes(1))
    m.setContent(canvas.id, newerSnapshot)
    finishSave?.({ revision: canvasRevision, canonicalPath: '/tmp/board.canvas' })
    await saving

    expect(canvas.initialContent).not.toBe(newerSnapshot)
    expect(canvas.currentContent).toBe(newerSnapshot)
    expect(m.isDirty(canvas.id)).toBe(true)
  })

  it('serializes Canvas Save As behind an in-flight save and keeps the new identity', async () => {
    const oldSavedRevision = { mtimeNs: '1700000001000000000', size: 28, sha256: 'old-saved' }
    const targetRevision = { mtimeNs: '1700000002000000000', size: 29, sha256: 'target-before' }
    const targetSavedRevision = { mtimeNs: '1700000003000000000', size: 30, sha256: 'target-saved' }
    let finishOldSave: ((value: { revision: typeof canvasRevision; canonicalPath: string }) => void) | undefined
    canvasSave
      .mockImplementationOnce(() => new Promise((resolve) => { finishOldSave = resolve }))
      .mockResolvedValueOnce({ revision: targetSavedRevision, canonicalPath: '/tmp/copy.canvas' })
    canvasProbe.mockResolvedValueOnce({
      kind: 'present', revision: targetRevision,
      requestedPath: '/tmp/copy.canvas', canonicalPath: '/tmp/copy.canvas',
    })
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    m.setContent(canvas.id, '{"nodes":[],"edges":[],"save":1}')

    const saving = m.saveActive()
    await vi.waitFor(() => expect(canvasSave).toHaveBeenCalledTimes(1))
    const savingAs = m.saveAs(canvas.id, '/tmp/copy.canvas')
    await Promise.resolve()
    expect(canvasProbe).not.toHaveBeenCalled()

    finishOldSave?.({ revision: oldSavedRevision, canonicalPath: '/tmp/board.canvas' })
    await saving
    await savingAs

    expect(canvasSave).toHaveBeenNthCalledWith(1, '/tmp/board.canvas', expect.any(String), canvasRevision, false)
    expect(canvasSave).toHaveBeenNthCalledWith(2, '/tmp/copy.canvas', expect.any(String), targetRevision, undefined)
    expect(canvas.filePath).toBe('/tmp/copy.canvas')
    expect(canvas.title).toBe('copy.canvas')
    expect(canvas.canvasRevision).toEqual(targetSavedRevision)
  })

  it('drops an autosave snapshot whose captured path became stale while queued', async () => {
    const targetRevision = { mtimeNs: '1700000002000000000', size: 29, sha256: 'target-before' }
    const targetSavedRevision = { mtimeNs: '1700000003000000000', size: 30, sha256: 'target-saved' }
    let finishSaveAs: ((value: { revision: typeof canvasRevision; canonicalPath: string }) => void) | undefined
    canvasProbe.mockResolvedValueOnce({
      kind: 'present', revision: targetRevision,
      requestedPath: '/tmp/copy.canvas', canonicalPath: '/tmp/copy.canvas',
    })
    canvasSave.mockImplementationOnce(() => new Promise((resolve) => { finishSaveAs = resolve }))
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    const oldPath = canvas.filePath

    const savingAs = m.saveAs(canvas.id, '/tmp/copy.canvas')
    await vi.waitFor(() => expect(canvasSave).toHaveBeenCalledTimes(1))
    const staleAutosave = m.persistCanvasSnapshot(canvas, '{"stale":true}', false, oldPath)
    finishSaveAs?.({ revision: targetSavedRevision, canonicalPath: '/tmp/copy.canvas' })
    await savingAs
    await staleAutosave

    expect(canvas.filePath).toBe('/tmp/copy.canvas')
    expect(canvas.canvasRevision).toEqual(targetSavedRevision)
    expect(canvasSave).toHaveBeenCalledTimes(1)
  })

  it('does not let an old Canvas save completion restore a rebound identity', async () => {
    const reboundRevision = { mtimeNs: '1700000004000000000', size: 31, sha256: 'rebound' }
    let finishSave: ((value: { revision: typeof canvasRevision; canonicalPath: string }) => void) | undefined
    canvasSave.mockImplementationOnce(() => new Promise((resolve) => { finishSave = resolve }))
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    m.setContent(canvas.id, '{"nodes":[],"edges":[],"save":1}')

    const saving = m.saveActive()
    await vi.waitFor(() => expect(canvasSave).toHaveBeenCalledTimes(1))
    canvas.filePath = '/tmp/rebound.canvas'
    canvas.title = 'rebound.canvas'
    canvas.canvasRevision = reboundRevision
    finishSave?.({ revision: canvasRevision, canonicalPath: '/tmp/board.canvas' })
    await saving

    expect(canvas.filePath).toBe('/tmp/rebound.canvas')
    expect(canvas.title).toBe('rebound.canvas')
    expect(canvas.canvasRevision).toEqual(reboundRevision)
  })

  it('exports a Canvas copy without changing the open tab identity or dirty baseline', async () => {
    canvasProbe.mockResolvedValueOnce({
      kind: 'missing', requestedPath: '/tmp/export.canvas', canonicalPath: '/tmp/export.canvas',
    })
    const watcher = await import('./file-watcher.svelte')
    const settings = await import('./settings.svelte')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    const edited = '{"nodes":[],"edges":[],"edited":true}'
    m.setContent(canvas.id, edited)
    const identityBefore = {
      filePath: canvas.filePath,
      title: canvas.title,
      revision: canvas.canvasRevision,
      initialContent: canvas.initialContent,
      lastKnownMtime: canvas.lastKnownMtime,
      lastKnownHash: canvas.lastKnownHash,
      externalState: canvas.externalState,
    }

    await m.exportCanvasCopy(canvas.id, '/tmp/export.canvas')

    expect(canvasCreate).toHaveBeenCalledWith('/tmp/export.canvas', edited)
    expect(canvas).toMatchObject({
      filePath: identityBefore.filePath,
      title: identityBefore.title,
      canvasRevision: identityBefore.revision,
      initialContent: identityBefore.initialContent,
      lastKnownMtime: identityBefore.lastKnownMtime,
      lastKnownHash: identityBefore.lastKnownHash,
      externalState: identityBefore.externalState,
    })
    expect(m.isDirty(canvas.id)).toBe(true)
    expect(watcher.rebindTabPath).not.toHaveBeenCalled()
    expect(settings.pushRecentFile).toHaveBeenCalledTimes(1)
  })

  it('surfaces a revision conflict as an external canvas change and keeps the local buffer', async () => {
    const actualRevision = { mtimeNs: '1700000001000000000', size: 31, sha256: 'external-hash' }
    canvasSave.mockRejectedValueOnce({
      kind: 'conflict', message: 'canvas changed on disk',
      expected: { kind: 'present', revision: canvasRevision },
      actual: { kind: 'present', revision: actualRevision },
      canonicalPath: '/tmp/board.canvas',
    })
    canvasOpen
      .mockResolvedValueOnce({
        text: '{"nodes":[],"edges":[]}', revision: canvasRevision,
        requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas',
      })
      .mockResolvedValueOnce({
        text: '{"nodes":[],"edges":[],"external":true}', revision: actualRevision,
        requestedPath: '/tmp/board.canvas', canonicalPath: '/tmp/board.canvas',
      })
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')
    const canvas = m.tabs[0]
    m.setContent(canvas.id, '{"nodes":[],"edges":[],"local":true}')

    await expect(m.saveActive()).rejects.toMatchObject({ kind: 'conflict' })

    expect(canvas.externalState).toBe('changed')
    expect(canvas.pendingExternal).toMatchObject({
      content: '{"nodes":[],"edges":[],"external":true}',
      hash: 'external-hash',
    })
    expect(canvas.currentContent).toContain('"local":true')
  })

  it('setContent toggles dirty correctly', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'new content')
    expect(m.isDirty(id)).toBe(true)
    m.setContent(id, m.tabs[0].initialContent)
    expect(m.isDirty(id)).toBe(false)
  })

  it('saveActive writes current content and updates baseline', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    expect(m.isDirty(id)).toBe(true)
    await m.saveActive()
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/foo.md', 'edited')
    expect(m.isDirty(id)).toBe(false)
  })

  it('saveActive refuses to write when externalState is "changed"', async () => {
    // The banner provides the explicit reconciliation UI (Reload / Overwrite /
    // Save as…). A blind ⌘S during this state would silently clobber the
    // external change — so saveActive must refuse and let the caller surface
    // a useful error.
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    m.setContent(t.id, 'mine')
    t.externalState = 'changed'
    t.pendingExternal = { mtime: 5000, hash: 'h-X', content: 'theirs' }
    await expect(m.saveActive()).rejects.toThrow(/external/i)
    expect(fs.writeMd).not.toHaveBeenCalled()
    // State must not have been mutated.
    expect(t.externalState).toBe('changed')
    expect(t.currentContent).toBe('mine')
  })

  it('saveActive still works when externalState is "deleted" (Recreate-on-Save)', async () => {
    // The deleted state has no external content to clobber — the file is
    // gone, and the banner's "Recreate on Save (⌘S)" button explicitly
    // delegates here. Only 'changed' is blocked.
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    m.setContent(t.id, 'recreated body')
    t.externalState = 'deleted'
    await m.saveActive()
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/foo.md', 'recreated body')
    expect(t.externalState).toBe('fresh')
  })

  it('closeTab removes when not dirty without prompt', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    const ok = await m.closeTab(id, async () => 'cancel')
    expect(ok).toBe(true)
    expect(m.tabs.length).toBe(0)
    expect(m.activeId.value).toBe(null)
  })

  // ── NAMED dirty file: uses the confirm() callback ───────────────────────────
  it('closeTab named dirty → confirm=save → saves to same path and closes', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    const ok = await m.closeTab(id, async () => 'save')
    expect(ok).toBe(true)
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/foo.md', 'edited')
    expect(m.tabs.length).toBe(0)
  })

  it('closeTab named dirty → confirm=discard → closes without saving', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    const ok = await m.closeTab(id, async () => 'discard')
    expect(ok).toBe(true)
    expect(fs.writeMd).not.toHaveBeenCalled()
    expect(m.tabs.length).toBe(0)
  })

  it('closeTab named dirty → confirm=cancel → tab stays', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    const ok = await m.closeTab(id, async () => 'cancel')
    expect(ok).toBe(false)
    expect(m.tabs.length).toBe(1)
  })

  it('closeTab named dirty passes the basename to the confirm callback', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    const confirmSpy = vi.fn(async () => 'discard' as const)
    await m.closeTab(id, confirmSpy)
    expect(confirmSpy).toHaveBeenCalledWith('foo.md')
  })

  // ── UNTITLED dirty file: goes straight to NSSavePanel ───────────────────────
  it('closeTab untitled dirty → user picks save path → saves and closes', async () => {
    const dialogs = await import('./dialogs')
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    vi.mocked(dialogs.pickSaveFile).mockResolvedValueOnce('/tmp/saved.md')
    m.newFile()
    const id = m.tabs[0].id
    m.setContent(id, 'new content')
    const ok = await m.closeTab(id, async () => 'cancel')
    expect(ok).toBe(true)
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/saved.md', 'new content')
    expect(m.tabs.length).toBe(0)
  })

  it('closeTab untitled dirty → cancels save panel + keeps editing → tab stays', async () => {
    const dialogs = await import('./dialogs')
    const tauri = await import('@tauri-apps/plugin-dialog')
    const m = await import('./tabs.svelte')
    vi.mocked(dialogs.pickSaveFile).mockResolvedValueOnce(null)
    vi.mocked(tauri.ask).mockResolvedValueOnce(false)  // Cancel (keep editing)
    m.newFile()
    const id = m.tabs[0].id
    m.setContent(id, 'new content')
    const ok = await m.closeTab(id, async () => 'cancel')
    expect(ok).toBe(false)
    expect(m.tabs.length).toBe(1)
  })

  it('closeTab untitled dirty → cancels save panel + discards → closes without saving', async () => {
    const dialogs = await import('./dialogs')
    const tauri = await import('@tauri-apps/plugin-dialog')
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    vi.mocked(dialogs.pickSaveFile).mockResolvedValueOnce(null)
    vi.mocked(tauri.ask).mockResolvedValueOnce(true)  // Don't Save (close)
    m.newFile()
    const id = m.tabs[0].id
    m.setContent(id, 'new content')
    const ok = await m.closeTab(id, async () => 'cancel')
    expect(ok).toBe(true)
    expect(fs.writeMd).not.toHaveBeenCalled()
    expect(m.tabs.length).toBe(0)
  })

  it('closing active tab activates a sibling', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/a.md')
    await m.openFile('/tmp/b.md')
    await m.openFile('/tmp/c.md')
    const bId = m.tabs[1].id
    m.activate(bId)
    await m.closeTab(bId, async () => 'discard')
    expect(m.tabs.length).toBe(2)
    expect(m.activeId.value).toBe(m.tabs[1].id)  // C (originally idx 2, now idx 1 after splice)
  })

  it('toggleMode flips source ⇄ rich', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    expect(m.tabs[0].mode).toBe('rich')
    m.toggleMode(id)
    expect(m.tabs[0].mode).toBe('source')
    m.toggleMode(id)
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('closeTab dirty non-active named tab → save=same path restores original active', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/a.md')
    await m.openFile('/tmp/b.md')
    await m.openFile('/tmp/c.md')
    const aId = m.tabs[0].id
    const bId = m.tabs[1].id
    m.activate(aId)             // A is active
    m.setContent(bId, 'edited') // B dirty
    const ok = await m.closeTab(bId, async () => 'save')
    expect(ok).toBe(true)
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/b.md', 'edited')
    expect(m.tabs.length).toBe(2)
    expect(m.activeId.value).toBe(aId)  // A still active, NOT C
  })

  it('saveAs renames path, updates title/baseline, clears dirty, persists mode', async () => {
    const fs = await import('./fs')
    const settings = await import('./settings.svelte')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.toggleMode(id)              // rich (default) → source
    m.setContent(id, 'edited')
    expect(m.isDirty(id)).toBe(true)
    await m.saveAs(id, '/tmp/bar.md')
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/bar.md', 'edited')
    expect(m.tabs[0].filePath).toBe('/tmp/bar.md')
    expect(m.tabs[0].title).toBe('bar.md')
    expect(m.isDirty(id)).toBe(false)
    expect(settings.pushRecentFile).toHaveBeenCalledWith('/tmp/bar.md')
    // Allow setRecentMode to flush
    await new Promise((r) => setTimeout(r, 0))
    expect(settings.setRecentMode).toHaveBeenCalledWith('md', 'source')
  })

  it('openFile uses stored mode for extension', async () => {
    const settings = await import('./settings.svelte')
    ;(settings.getRecentMode as unknown as ReturnType<typeof vi.fn>).mockReturnValueOnce('rich')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('openFile defaults to rich when no stored mode', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('openPathBackedMarkdownDraft uses the stored mode for the extension', async () => {
    const settings = await import('./settings.svelte')
    ;(settings.getRecentMode as unknown as ReturnType<typeof vi.fn>).mockReturnValueOnce('source')
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/tmp/quick.md', '', { skipEmptySave: true })
    expect(m.tabs[0].mode).toBe('source')
  })

  it('openPathBackedMarkdownDraft honours an explicit mode over the stored one', async () => {
    const settings = await import('./settings.svelte')
    const stored = settings.getRecentMode as unknown as ReturnType<typeof vi.fn>
    // Not `mockReturnValueOnce`: an explicit mode short-circuits the lookup, so a
    // queued value would go unconsumed and leak into the next test.
    stored.mockReturnValue('source')
    try {
      const m = await import('./tabs.svelte')
      await m.openPathBackedMarkdownDraft('/tmp/note.md', '', { mode: 'rich' })
      expect(m.tabs[0].mode).toBe('rich')
    } finally {
      stored.mockReturnValue(null)
    }
  })

  it('saveActive renames a titled quick note after its H1', async () => {
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/vault/inbox/2026-07-25-193045-quick.md', '', {
      skipEmptySave: true,
    })
    m.setContent(m.tabs[0].id, '# 产品思考\n\nbody')
    await m.saveActive()
    expect(fsRename).toHaveBeenCalledWith(
      '/vault/inbox/2026-07-25-193045-quick.md',
      '/vault/inbox/2026-07-25-产品思考.md',
    )
    expect(m.tabs[0].filePath).toBe('/vault/inbox/2026-07-25-产品思考.md')
    expect(m.tabs[0].title).toBe('2026-07-25-产品思考.md')
  })

  it('the auto-save path holds off until the title line is finished', async () => {
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/vault/inbox/2026-07-25-193045-quick.md', '', {
      skipEmptySave: true,
    })
    const t = m.tabs[0]
    // Mid-typing: an 800 ms auto-save must not freeze a partial heading.
    m.setContent(t.id, '# 产品')
    await m.renameAutoQuickNoteIfTitled(t, true)
    expect(fsRename).not.toHaveBeenCalled()
    // Enter pressed → the title is settled and the rename lands.
    m.setContent(t.id, '# 产品思考\n')
    await m.renameAutoQuickNoteIfTitled(t, true)
    expect(fsRename).toHaveBeenCalledWith(
      '/vault/inbox/2026-07-25-193045-quick.md',
      '/vault/inbox/2026-07-25-产品思考.md',
    )
  })

  it('saveActive leaves an untitled quick note under its generated name', async () => {
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/vault/inbox/2026-07-25-193045-quick.md', '', {
      skipEmptySave: true,
    })
    m.setContent(m.tabs[0].id, 'no heading, just text')
    await m.saveActive()
    expect(fsRename).not.toHaveBeenCalled()
    expect(m.tabs[0].filePath).toBe('/vault/inbox/2026-07-25-193045-quick.md')
  })

  it('quick-note rename sidesteps an existing file instead of clobbering it', async () => {
    fsExists.mockImplementation(
      async (p: unknown) => p === '/vault/inbox/2026-07-25-产品思考.md',
    )
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/vault/inbox/2026-07-25-193045-quick.md', '', {
      skipEmptySave: true,
    })
    m.setContent(m.tabs[0].id, '# 产品思考')
    await m.saveActive()
    expect(fsRename).toHaveBeenCalledWith(
      '/vault/inbox/2026-07-25-193045-quick.md',
      '/vault/inbox/2026-07-25-产品思考-2.md',
    )
  })

  it('a failed quick-note rename leaves the saved file in place', async () => {
    fsRename.mockRejectedValueOnce(new Error('EPERM'))
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/vault/inbox/2026-07-25-193045-quick.md', '', {
      skipEmptySave: true,
    })
    m.setContent(m.tabs[0].id, '# 产品思考')
    await expect(m.saveActive()).resolves.toBeUndefined()
    expect(m.tabs[0].filePath).toBe('/vault/inbox/2026-07-25-193045-quick.md')
  })

  it('setMode persists choice keyed by extension', async () => {
    const settings = await import('./settings.svelte')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    m.toggleMode(m.tabs[0].id)   // rich (default) → source
    await new Promise((r) => setTimeout(r, 0))
    expect(settings.setRecentMode).toHaveBeenCalledWith('md', 'source')
  })

  it('openFile classifies markdown', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    expect(m.tabs[0].kind).toBe('markdown')
    expect(m.tabs[0].language).toBeUndefined()
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('openFile classifies html with default rich mode', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/index.html')
    expect(m.tabs[0].kind).toBe('html')
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('openFile classifies code with language', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/script.py')
    expect(m.tabs[0].kind).toBe('code')
    expect(m.tabs[0].language).toBe('python')
    expect(m.tabs[0].mode).toBe('rich')
  })

  it('openFile rejects binary content', async () => {
    const fs = await import('./fs')
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('plain\x00text')
    const m = await import('./tabs.svelte')
    await expect(m.openFile('/tmp/foo.md')).rejects.toThrow(/binary/i)
    expect(m.tabs.length).toBe(0)
  })

  it('saveAs reclassifies tab when extension changes', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    await m.saveAs(id, '/tmp/foo.py')
    expect(m.tabs[0].kind).toBe('code')
    expect(m.tabs[0].language).toBe('python')
    expect(m.tabs[0].title).toBe('foo.py')
  })

  it('openFile populates externalState/lastKnownMtime/lastKnownHash', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    expect(t.externalState).toBe('fresh')
    expect(t.externalBannerDismissed).toBe(false)
    expect(typeof t.lastKnownMtime).toBe('number')
    expect(t.lastKnownHash).toMatch(/^[0-9a-f]{64}$/)
    expect(t.pendingExternal).toBeUndefined()
  })

  it('saveActive updates lastKnownMtime/lastKnownHash to post-write values', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const id = m.tabs[0].id
    m.setContent(id, 'edited')
    // Queue the post-write stat result so recordOurWrite captures it.
    ;(fs.statFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      mtime: 9_999_999_999_999, size: 7,
    })
    await m.saveActive()
    const t = m.tabs.find((x) => x.id === id)!
    expect(t.lastKnownMtime).toBe(9_999_999_999_999)
    expect(t.lastKnownHash).toMatch(/^[0-9a-f]{64}$/)
    // After save, hash must be the hash of "edited"
    const { sha256Hex } = await import('./hash')
    expect(t.lastKnownHash).toBe(await sha256Hex('edited'))
  })

  it('reloadFromDisk replaces buffer with pendingExternal content and clears banner', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    // Simulate banner shown:
    m.setContent(t.id, 'edited')
    t.externalState = 'changed'
    t.pendingExternal = { mtime: 5000, hash: 'h-X', content: 'NEW DISK' }
    await m.reloadFromDisk(t.id)
    expect(t.currentContent).toBe('NEW DISK')
    expect(t.initialContent).toBe('NEW DISK')
    expect(t.externalState).toBe('fresh')
    expect(t.lastKnownMtime).toBe(5000)
    expect(t.lastKnownHash).toBe('h-X')
    expect(t.pendingExternal).toBeUndefined()
  })

  it('overwriteOnDisk writes the local buffer and clears banner', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    m.setContent(t.id, 'mine')
    t.externalState = 'changed'
    t.pendingExternal = { mtime: 5000, hash: 'h-X', content: 'theirs' }
    await m.overwriteOnDisk(t.id)
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/foo.md', 'mine')
    expect(t.externalState).toBe('fresh')
    expect(t.pendingExternal).toBeUndefined()
  })

  it('dismissExternalBanner sets the flag without changing externalState', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    t.externalState = 'changed'
    m.dismissExternalBanner(t.id)
    expect(t.externalBannerDismissed).toBe(true)
    expect(t.externalState).toBe('changed')
  })

  it('openFile spreadsheet (csv): kind=spreadsheet, mode=rich', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/data.csv')
    const t = m.tabs[0]
    expect(t.kind).toBe('spreadsheet')
    expect(t.mode).toBe('rich')
    expect(t.currentContent).toContain('content of /tmp/data.csv')
  })

  it('openFile tsv: kind=code (tab-delimited not yet implemented), mode=rich', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/data.tsv')
    const t = m.tabs[0]
    expect(t.kind).toBe('code')
    expect(t.mode).toBe('rich')
  })

  it('openFile image: kind=image, currentContent empty, mode=rich', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/photo.png')
    expect(m.tabs.length).toBe(1)
    const t = m.tabs[0]
    expect(t.kind).toBe('image')
    expect(t.currentContent).toBe('')
    expect(t.initialContent).toBe('')
    expect(t.mode).toBe('rich')
    expect(m.isDirty(t.id)).toBe(false)
    // readMd should NOT have been called for an image
    expect(fs.readMd).not.toHaveBeenCalled()
  })

  // ── newFile ─────────────────────────────────────────────────────────────────
  it('newCanvas creates a named Obsidian-compatible .canvas document', async () => {
    canvasCreate.mockClear()
    const dialogs = await import('./dialogs')
    const m = await import('./tabs.svelte')
    ;(dialogs.pickSaveCanvasFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce('/tmp/board.canvas')
    await m.newCanvas()
    expect(canvasCreate).toHaveBeenCalledWith('/tmp/board.canvas', m.EMPTY_CANVAS_CONTENT)
    expect(m.tabs[0]).toMatchObject({
      filePath: '/tmp/board.canvas', title: 'board.canvas', kind: 'canvas', mode: 'rich',
    })
  })

  it('newCanvas creates no tab and writes nothing when save is cancelled', async () => {
    const fs = await import('./fs')
    const dialogs = await import('./dialogs')
    ;(dialogs.pickSaveCanvasFile as ReturnType<typeof vi.fn>).mockResolvedValueOnce(null)
    const m = await import('./tabs.svelte')
    await m.newCanvas()
    expect(fs.writeMd).not.toHaveBeenCalled()
    expect(m.tabs).toHaveLength(0)
  })

  it('newFile creates an untitled markdown tab, dirty from the start', async () => {
    const m = await import('./tabs.svelte')
    m.newFile()
    expect(m.tabs.length).toBe(1)
    const t = m.tabs[0]
    expect(t.filePath).toBe('')
    expect(t.title).toBe('untitled.md')
    expect(t.kind).toBe('markdown')
    expect(t.initialContent).toBe('')
    expect(t.currentContent).not.toBe('')  // random template
    expect(m.isDirty(t.id)).toBe(true)
    expect(m.activeId.value).toBe(t.id)
  })

  it('newFile inherits mode from the currently active non-image tab', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    m.toggleMode(m.tabs[0].id)   // rich (default) → source
    m.newFile()
    expect(m.tabs[1].mode).toBe('source')
  })

  it('newFile falls back to source mode when no tab is open', async () => {
    const m = await import('./tabs.svelte')
    m.newFile()
    expect(m.tabs[0].mode).toBe('source')
  })

  it('newFile signs the doc via humanActorNow() when the identity cache is warm', async () => {
    // Wiring test (not a newFileText unit test): drives the real newFile()
    // call site with a warm identity cache and asserts the signature reached
    // the tab content through it — catches a renamed { by, at } shape or a
    // dropped conditional that a hand-built-author unit test cannot.
    humanActorNowMock.mockReturnValue('human:testuser')
    const m = await import('./tabs.svelte')
    m.newFile()
    expect(m.tabs[0].currentContent).toContain('generated:\n  by: human:testuser\n  at:')
  })

  it('newFile writes no generated key when the identity cache is cold', async () => {
    // humanActorNowMock defaults to null (see beforeEach) — a cold cache must
    // not produce a guessed signature.
    const m = await import('./tabs.svelte')
    m.newFile()
    expect(m.tabs[0].currentContent).not.toContain('generated:')
  })

  it('path-backed markdown draft skips empty saves but writes non-empty content', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/tmp/inbox/quick.md', '', { skipEmptySave: true })
    const t = m.tabs[0]
    expect(t.filePath).toBe('/tmp/inbox/quick.md')
    expect(t.currentContent).toBe('')
    expect(m.isDirty(t.id)).toBe(false)

    await m.saveActive()
    expect(fs.writeMd).not.toHaveBeenCalled()

    m.setContent(t.id, 'hello')
    await m.saveActive()
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/inbox/quick.md', 'hello')
    expect(m.isDirty(t.id)).toBe(false)
  })

  it('path-backed markdown draft does not save over an existing file with empty content', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openPathBackedMarkdownDraft('/tmp/inbox/quick.md', '', { skipEmptySave: true })
    const t = m.tabs[0]
    m.setContent(t.id, 'hello')
    await m.saveActive()
    ;(fs.writeMd as ReturnType<typeof vi.fn>).mockClear()

    m.setContent(t.id, '')
    await m.saveActive()
    expect(fs.writeMd).not.toHaveBeenCalled()
    expect(m.isDirty(t.id)).toBe(true)
  })

  it('newFile dispatches notemd:new-file-select when window is available', async () => {
    const dispatched: CustomEvent[] = []
    ;(globalThis as Record<string, unknown>).window = {
      dispatchEvent: (e: CustomEvent) => dispatched.push(e),
    }
    try {
      const m = await import('./tabs.svelte')
      m.newFile()
      await new Promise((r) => setTimeout(r, 0))  // flush queueMicrotask
      expect(dispatched.length).toBe(1)
      expect(dispatched[0].type).toBe('notemd:new-file-select')
      expect(dispatched[0].detail.start).toBeGreaterThan(0)
      expect(dispatched[0].detail.end).toBeGreaterThan(dispatched[0].detail.start)
    } finally {
      delete (globalThis as Record<string, unknown>).window
    }
  })

  it('openFile image: isDirty always false even after setContent', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/photo.jpg')
    const t = m.tabs[0]
    // Even if somehow content were set, isDirty stays false because initialContent=''
    expect(m.isDirty(t.id)).toBe(false)
  })

  it('openFile image: lastKnownMtime populated from stat', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/img.svg')
    const t = m.tabs[0]
    expect(t.lastKnownMtime).toBe(1_700_000_000_000)
    expect(t.lastKnownHash).toBe('')
  })

  it('updateTabPath rebinds filePath and title without touching content', async () => {
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/old.md')
    const tab = m.tabs.find((t: { filePath: string }) => t.filePath === '/tmp/old.md')!
    m.setContent(tab.id, 'edited')
    await m.updateTabPath('/tmp/old.md', '/tmp/new.md')
    expect(tab.filePath).toBe('/tmp/new.md')
    expect(tab.title).toBe('new.md')
    expect(tab.currentContent).toBe('edited')
  })

  it('updateTabPath rewrites exact references in open canvases and preserves extensions', async () => {
    canvasOpen.mockResolvedValueOnce({
      text: JSON.stringify({
        nodes: [
          { id: 'f', type: 'file', file: 'asset.png', x: 0, y: 0, width: 100, height: 100, vendor: 7 },
          { id: 'g', type: 'group', label: 'G', background: 'asset.png', x: 0, y: 0, width: 200, height: 200 },
        ],
        edges: [],
      }),
      revision: canvasRevision,
      requestedPath: '/tmp/board.canvas',
      canonicalPath: '/tmp/board.canvas',
    })
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/board.canvas')

    await m.updateTabPath('/tmp/asset.png', '/tmp/renamed.png')

    const saved = JSON.parse(m.tabs[0].currentContent)
    expect(saved.nodes[0]).toMatchObject({ file: 'renamed.png', vendor: 7 })
    expect(saved.nodes[1]).toMatchObject({ background: 'renamed.png' })
    expect(m.isDirty(m.tabs[0].id)).toBe(true)
  })
  it('updateTabPath is a no-op when no tab has the path', async () => {
    const m = await import('./tabs.svelte')
    await expect(m.updateTabPath('/tmp/nope.md', '/tmp/x.md')).resolves.toBeUndefined()
  })

  // ── restoreVersion (git history "Restore this version") ──────────────────────
  it('restoreVersion writes the old content to disk and lands the tab clean', async () => {
    // Restore = confirm rollback: persist immediately and clear dirty, so the
    // user never has to press ⌘S. The buffer is re-read from disk (auto-reload
    // path), so currentContent === initialContent === the restored bytes.
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    m.setContent(t.id, 'user edits')          // dirty with unrelated content
    expect(m.isDirty(t.id)).toBe(true)
    ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('OLD VERSION')
    await m.restoreVersion(t.id, 'OLD VERSION')
    expect(fs.writeMd).toHaveBeenCalledWith('/tmp/foo.md', 'OLD VERSION')
    expect(t.currentContent).toBe('OLD VERSION')
    expect(t.initialContent).toBe('OLD VERSION')
    expect(m.isDirty(t.id)).toBe(false)
  })

  it('restoreVersion dispatches notemd:auto-reloaded so every editor rebuilds', async () => {
    // OutlineEditor (and SourceView cursor-preserve) only refresh on this event;
    // reusing the auto-reload path is what makes restore visible in all modes.
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    await m.openFile('/tmp/foo.md')
    const t = m.tabs[0]
    const dispatched: CustomEvent[] = []
    ;(globalThis as Record<string, unknown>).window = {
      dispatchEvent: (e: CustomEvent) => dispatched.push(e),
    }
    try {
      ;(fs.readMd as ReturnType<typeof vi.fn>).mockResolvedValueOnce('V1')
      await m.restoreVersion(t.id, 'V1')
    } finally {
      delete (globalThis as Record<string, unknown>).window
    }
    const evt = dispatched.find((e) => e.type === 'notemd:auto-reloaded')
    expect(evt).toBeTruthy()
    expect(evt!.detail.tabId).toBe(t.id)
    expect(evt!.detail.newContent).toBe('V1')
  })

  it('restoreVersion is a no-op for an untitled (path-less) tab', async () => {
    const fs = await import('./fs')
    const m = await import('./tabs.svelte')
    m.newFile()
    const t = m.tabs[0]
    await m.restoreVersion(t.id, 'X')
    expect(fs.writeMd).not.toHaveBeenCalled()
  })
})

describe('shouldSkipEmptySave — 预置 frontmatter 的草稿仍算空', () => {
  it('treats a draft that is only an OKF concept head as empty', async () => {
    const { shouldSkipEmptySave } = await import('./tabs.svelte')
    const tab = { skipEmptySave: true, currentContent: '---\ntype: Note\n---\n' } as never
    expect(shouldSkipEmptySave(tab)).toBe(true)
  })
  it('saves once the user has written a body', async () => {
    const { shouldSkipEmptySave } = await import('./tabs.svelte')
    const tab = { skipEmptySave: true, currentContent: '---\ntype: Note\n---\n# 标题\n' } as never
    expect(shouldSkipEmptySave(tab)).toBe(false)
  })
})
