import { describe, it, expect, vi } from 'vitest'

vi.mock('@tauri-apps/plugin-store', () => ({ Store: { load: vi.fn() } }))
vi.mock('../platform.svelte', () => ({ platform: () => Promise.resolve('macos') }))

const { outlineAppliesTo, isOutlineNoteTab, outlineGate } = await import('./gate.svelte')
outlineGate.enabled = true // set at app startup; tests skip that path

describe('outlineAppliesTo', () => {
  it('applies to markdown', () => {
    expect(outlineAppliesTo({ kind: 'markdown', filePath: '/d/a.md' })).toBe(true)
  })

  it('applies to mdx — sidecar notes are the only way to annotate it', () => {
    // mdx opens read-only in rich mode, so the companion note is where a
    // reader's judgement goes. Without this the panel would never show.
    expect(outlineAppliesTo({ kind: 'mdx', filePath: '/d/guide.mdx' })).toBe(true)
  })

  it('does not apply to code or images', () => {
    expect(outlineAppliesTo({ kind: 'code', filePath: '/d/a.ts' })).toBe(false)
    expect(outlineAppliesTo({ kind: 'image', filePath: '/d/a.png' })).toBe(false)
  })

  it('the mdx companion itself is a full outline tab, not a host', () => {
    expect(outlineAppliesTo({ kind: 'markdown', filePath: '/d/guide.mdx.note.md' })).toBe(false)
    expect(isOutlineNoteTab({ kind: 'markdown', filePath: '/d/guide.mdx.note.md' })).toBe(true)
  })
})
