import { describe, it, expect, vi } from 'vitest'

vi.mock('@tauri-apps/plugin-store', () => ({ Store: { load: vi.fn() } }))
vi.mock('../platform.svelte', () => ({ platform: () => Promise.resolve('macos') }))

const { outlineAppliesTo, outlineGate } = await import('./gate.svelte')
outlineGate.enabled = true // set at app startup; tests skip that path

describe('outlineAppliesTo', () => {
  it('applies to markdown', () => {
    expect(outlineAppliesTo({ kind: 'markdown', filePath: '/d/a.md' })).toBe(true)
  })

  it('does not apply to mdx — read-only rendering is the whole support surface', () => {
    expect(outlineAppliesTo({ kind: 'mdx', filePath: '/d/guide.mdx' })).toBe(false)
  })

  it('does not apply to code or images', () => {
    expect(outlineAppliesTo({ kind: 'code', filePath: '/d/a.ts' })).toBe(false)
    expect(outlineAppliesTo({ kind: 'image', filePath: '/d/a.png' })).toBe(false)
  })

})
