/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { bridgeMediaResolver, loadVaultRoot } from './media'

function stubBridge(request: unknown) {
  ;(window as unknown as { notemd: unknown }).notemd = { request }
}

beforeEach(() => {
  delete (window as unknown as { notemd?: unknown }).notemd
})

describe('bridgeMediaResolver', () => {
  it('reads a vault-hosted absolute path through host.vault.read_bytes and returns a blob url', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' }) // "hello"
    stubBridge(request)
    const r = bridgeMediaResolver('/vault')
    const url = await r.loadLocalImage('/vault/inbox/ideas/img.png')
    expect(request).toHaveBeenCalledWith('host.vault.read_bytes', { path: 'inbox/ideas/img.png' })
    expect(url).toMatch(/^blob:/)
  })

  it('caches by absolute path so the same file is read once', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' })
    stubBridge(request)
    const r = bridgeMediaResolver('/vault')
    const a = await r.loadLocalImage('/vault/a.png')
    const b = await r.loadLocalImage('/vault/a.png')
    expect(a).toBe(b)
    expect(request).toHaveBeenCalledTimes(1)
  })

  it('returns an empty string for paths outside the vault, without touching the bridge', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' })
    stubBridge(request)
    const r = bridgeMediaResolver('/vault')
    expect(await r.loadLocalImage('/elsewhere/img.png')).toBe('')
    expect(await r.loadLocalMedia('/vaultish/clip.mp4')).toBe('')
    expect(request).not.toHaveBeenCalled()
  })

  it('returns an empty string when the vault root is unknown', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' })
    stubBridge(request)
    const r = bridgeMediaResolver('')
    expect(await r.loadLocalImage('/vault/img.png')).toBe('')
    expect(request).not.toHaveBeenCalled()
  })

  it('returns an empty string when the bridge read fails', async () => {
    stubBridge(vi.fn().mockRejectedValue(new Error('io')))
    const r = bridgeMediaResolver('/vault')
    expect(await r.loadLocalImage('/vault/missing.png')).toBe('')
  })

  it('returns an empty string when there is no bridge at all', async () => {
    const r = bridgeMediaResolver('/vault')
    expect(await r.loadLocalImage('/vault/img.png')).toBe('')
  })

  it('resolves audio/video through the same bridge call', async () => {
    const request = vi.fn().mockResolvedValue({ base64: 'aGVsbG8=' })
    stubBridge(request)
    const r = bridgeMediaResolver('/vault')
    const url = await r.loadLocalMedia('/vault/media/clip.mp4')
    expect(request).toHaveBeenCalledWith('host.vault.read_bytes', { path: 'media/clip.mp4' })
    expect(url).toMatch(/^blob:/)
  })

  it('passes remote urls through untouched', async () => {
    stubBridge(vi.fn())
    const r = bridgeMediaResolver('/vault')
    expect(await r.loadRemoteMedia('https://a/b.png')).toBe('https://a/b.png')
  })
})

describe('loadVaultRoot', () => {
  it('returns the root reported by host.vault.info', async () => {
    const request = vi.fn().mockResolvedValue({ root: '/vault', wiki_dir: 'wiki', daily_dir: 'daily' })
    stubBridge(request)
    expect(await loadVaultRoot()).toBe('/vault')
    expect(request).toHaveBeenCalledWith('host.vault.info', {})
  })

  it('returns an empty string when no vault is configured or the call fails', async () => {
    stubBridge(vi.fn().mockResolvedValue({ root: null }))
    expect(await loadVaultRoot()).toBe('')
    stubBridge(vi.fn().mockRejectedValue(new Error('denied')))
    expect(await loadVaultRoot()).toBe('')
  })
})
