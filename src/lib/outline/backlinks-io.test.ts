// src/lib/outline/backlinks-io.test.ts
//
// Wiring test for the vault-external branch of openPageOrCreate() — this is
// the call site that used to write a 0-byte .md for an unresolved
// [[wikilink]] clicked outside a vault (the RichEditor path is the other
// half of the same defect, covered separately since it lives in a .svelte
// component). Drives the real function with a mocked identity module and
// asserts the signature reached the text actually handed to writeTextFile().
import { describe, it, expect, vi, beforeEach } from 'vitest'

const exists = vi.fn()
const writeTextFile = vi.fn()
vi.mock('@tauri-apps/plugin-fs', () => ({
  exists: (...a: unknown[]) => exists(...a),
  writeTextFile: (...a: unknown[]) => writeTextFile(...a),
  watchImmediate: vi.fn(),
  readDir: vi.fn(),
  stat: vi.fn(),
  readTextFile: vi.fn(),
  remove: vi.fn(),
  mkdir: vi.fn(),
}))

const openFile = vi.fn()
vi.mock('../tabs.svelte', () => ({
  openFile: (...a: unknown[]) => openFile(...a),
}))

const humanActor = vi.fn()
vi.mock('../okf/identity', () => ({
  humanActor: (...a: unknown[]) => humanActor(...a),
}))

describe('openPageOrCreate — vault-external page creation (RichEditor 之外的另一半调用点)', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    exists.mockResolvedValue(false)
    writeTextFile.mockResolvedValue(undefined)
    openFile.mockResolvedValue(undefined)
    humanActor.mockResolvedValue('human:bruce')

    const { outline } = await import('./store.svelte')
    outline.docPath = '/outside/notes/index.md'
    outline.backlinkIndex = null

    const { sotvaultStore } = await import('../sotvault.svelte')
    sotvaultStore.vaultRoot = null
  })

  it('signs a vault-external page created from an unresolved [[wikilink]] — and it is not 0 bytes', async () => {
    const { openPageOrCreate } = await import('./backlinks-io.svelte')
    await openPageOrCreate('某新页')

    expect(writeTextFile).toHaveBeenCalledTimes(1)
    const [path, text] = writeTextFile.mock.calls[0] as [string, string]
    expect(path).toBe('/outside/notes/某新页.md')
    expect(text).toContain('generated:\n  by: human:bruce\n  at:')
    expect(text.length).toBeGreaterThan(0)
    expect(openFile).toHaveBeenCalledWith('/outside/notes/某新页.md')
  })

  it('writes no generated key when identity resolution fails (cold cache) — still not 0 bytes', async () => {
    humanActor.mockRejectedValue(new Error('no identity'))
    const { openPageOrCreate } = await import('./backlinks-io.svelte')
    await openPageOrCreate('某新页')

    const [, text] = writeTextFile.mock.calls[0] as [string, string]
    expect(text).not.toContain('generated')
    expect(text.length).toBeGreaterThan(0)
  })

  it('does not touch an existing file — no write, no signature to worry about', async () => {
    exists.mockResolvedValue(true)
    const { openPageOrCreate } = await import('./backlinks-io.svelte')
    await openPageOrCreate('某新页')

    expect(writeTextFile).not.toHaveBeenCalled()
    expect(openFile).toHaveBeenCalledWith('/outside/notes/某新页.md')
  })
})
