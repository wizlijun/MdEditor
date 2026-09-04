// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CanvasResourceSession, importCanvasResource, resolveCanvasResource } from './resource'

const h = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }))

describe('CanvasResourceSession', () => {
  beforeEach(() => {
    h.invoke.mockReset()
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:canvas-image')
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('loads local images only through the root-confined Rust command and caches the Blob URL', async () => {
    h.invoke.mockResolvedValue(Uint8Array.from([1, 137, 80, 78, 71]).buffer)
    const session = new CanvasResourceSession('/vault')

    expect(await session.loadLocalImage('/vault/assets/image.png')).toBe('blob:canvas-image')
    expect(await session.loadLocalImage('/vault/assets/image.png')).toBe('blob:canvas-image')
    expect(h.invoke).toHaveBeenCalledTimes(1)
    expect(h.invoke).toHaveBeenCalledWith('canvas_resource_read', {
      root: '/vault', target: '/vault/assets/image.png',
    })

    session.dispose()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:canvas-image')
  })

  it('denies remote/media loads and refuses a single entry over its byte budget', async () => {
    const session = new CanvasResourceSession('/vault', 3)
    expect(await session.loadRemoteMedia('https://example.com/tracker.png')).toBe('')
    expect(await session.loadLocalMedia('/vault/movie.mp4')).toBe('')
    expect(h.invoke).not.toHaveBeenCalled()

    h.invoke.mockResolvedValue(Uint8Array.from([1, 1, 2, 3, 4]).buffer)
    expect(await session.loadLocalImage('/vault/large.png')).toBe('')
    expect(URL.createObjectURL).not.toHaveBeenCalled()
  })

  it('fails closed for unexpected MIME values and backend errors', async () => {
    const session = new CanvasResourceSession('/vault')
    h.invoke.mockResolvedValueOnce(Uint8Array.from([255, 1]).buffer)
    expect(await session.loadLocalImage('/vault/x.svg')).toBe('')
    h.invoke.mockRejectedValueOnce({ kind: 'outsideRoot' })
    expect(await session.loadLocalImage('/outside/x.png')).toBe('')
  })

  it('evicts the least-recently-used blobs by byte budget and disposes the remainder', async () => {
    vi.mocked(URL.createObjectURL)
      .mockReturnValueOnce('blob:a')
      .mockReturnValueOnce('blob:b')
      .mockReturnValueOnce('blob:c')
    h.invoke.mockResolvedValue(Uint8Array.from([1, 1, 2, 3, 4]).buffer)
    const session = new CanvasResourceSession('/vault', 8)

    await session.loadLocalImage('/vault/a.png')
    await session.loadLocalImage('/vault/b.png')
    await session.loadLocalImage('/vault/a.png') // refresh A; B is now oldest
    await session.loadLocalImage('/vault/c.png')

    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:b')
    expect(session.peek('/vault/a.png')).toBe('blob:a')
    expect(session.peek('/vault/b.png')).toBeNull()
    expect(session.peek('/vault/c.png')).toBe('blob:c')

    session.dispose()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:a')
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:c')
  })
})

it('imports resources through the dedicated backend command', async () => {
  h.invoke.mockResolvedValue({
    relativePath: 'board_files/photo-2.png',
    canonicalPath: '/vault/board_files/photo-2.png',
    size: 12,
  })
  await expect(importCanvasResource('/vault', '/vault/board.canvas', '/tmp/photo.png'))
    .resolves.toMatchObject({ relativePath: 'board_files/photo-2.png' })
  expect(h.invoke).toHaveBeenCalledWith('canvas_resource_import', {
    root: '/vault', canvasPath: '/vault/board.canvas', sourcePath: '/tmp/photo.png',
  })
})

it('resolves file-node targets through backend containment before opening', async () => {
  h.invoke.mockResolvedValue({ canonicalPath: '/vault/archive.zip' })
  await expect(resolveCanvasResource('/vault', '/vault/archive.zip')).resolves.toBe('/vault/archive.zip')
  expect(h.invoke).toHaveBeenCalledWith('canvas_resource_resolve', {
    root: '/vault', target: '/vault/archive.zip',
  })
})
