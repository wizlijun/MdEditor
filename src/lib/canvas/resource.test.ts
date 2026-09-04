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

  it('limits concurrent reads and releases a slot when one finishes', async () => {
    const resolves: Array<(value: ArrayBuffer) => void> = []
    h.invoke.mockImplementation(() => new Promise<ArrayBuffer>((resolve) => resolves.push(resolve)))
    const session = new CanvasResourceSession('/vault')
    const active = Array.from({ length: 128 }, (_, index) => (
      session.loadLocalImage(`/vault/pending-${index}.png`)
    ))

    expect(h.invoke).toHaveBeenCalledTimes(128)
    await expect(session.loadLocalImage('/vault/overflow.png')).resolves.toBe('')
    expect(h.invoke).toHaveBeenCalledTimes(128)

    resolves[0](Uint8Array.from([1, 1]).buffer)
    await active[0]
    const afterRelease = session.loadLocalImage('/vault/after-release.png')
    expect(h.invoke).toHaveBeenCalledTimes(129)
    resolves[128](Uint8Array.from([1, 2]).buffer)
    await expect(afterRelease).resolves.toBe('blob:canvas-image')

    for (const resolve of resolves.slice(1, 128)) resolve(Uint8Array.from([1, 3]).buffer)
    await Promise.all(active)
    session.dispose()
  })

  it('releases failed request slots so an image can be retried', async () => {
    h.invoke.mockRejectedValue({ kind: 'temporaryFailure' })
    const session = new CanvasResourceSession('/vault')
    await Promise.all(Array.from({ length: 128 }, (_, index) => (
      session.loadLocalImage(`/vault/failed-${index}.png`)
    )))

    h.invoke.mockResolvedValueOnce(Uint8Array.from([1, 1, 2, 3]).buffer)
    await expect(session.loadLocalImage('/vault/failed-0.png')).resolves.toBe('blob:canvas-image')
    expect(h.invoke).toHaveBeenCalledTimes(129)
    session.dispose()
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

    vi.mocked(URL.createObjectURL).mockReturnValueOnce('blob:b-reloaded')
    expect(await session.loadLocalImage('/vault/b.png')).toBe('blob:b-reloaded')
    expect(h.invoke).toHaveBeenCalledTimes(4)
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:a')
    expect(session.peek('/vault/b.png')).toBe('blob:b-reloaded')

    session.dispose()
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:c')
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:b-reloaded')
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
