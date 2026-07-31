import { describe, it, expect } from 'vitest'
import { mergeIndexes, compareVersions } from './gen-plugin-index.mjs'

// Minimal RegistryEntry stand-in: mergeIndexes only keys on id/version and
// compares entries structurally; every other field rides along opaquely.
const entry = (id: string, version: string, extra: Record<string, unknown> = {}) => ({
  id,
  version,
  min_host: '>=0.0.0',
  ...extra,
})

describe('compareVersions', () => {
  it('orders dotted numeric versions numerically, not lexically', () => {
    expect(compareVersions('1.0.9', '1.0.10')).toBeLessThan(0)
    expect(compareVersions('1.0.10', '1.0.9')).toBeGreaterThan(0)
  })

  it('treats missing components as zero', () => {
    expect(compareVersions('1.0', '1.0.0')).toBe(0)
    expect(compareVersions('1.0.0', '1.0.0')).toBe(0)
  })
})

describe('mergeIndexes', () => {
  it('keeps live entries absent from the local build (the clobber case)', () => {
    const local = [entry('notemd.decision-log', '1.0.1')]
    const live = [entry('notemd.openclaw-chat', '1.0.0'), entry('notemd.pos-log', '1.0.0')]
    const r = mergeIndexes(local, live, [])
    const keys = r.plugins.map((p: { id: string; version: string }) => `${p.id}@${p.version}`)
    expect(keys).toContain('notemd.openclaw-chat@1.0.0')
    expect(keys).toContain('notemd.pos-log@1.0.0')
    expect(keys).toContain('notemd.decision-log@1.0.1')
    expect(r.kept).toEqual(['notemd.openclaw-chat@1.0.0', 'notemd.pos-log@1.0.0'])
  })

  it('prefers the local entry when the same id@version exists on both sides', () => {
    const local = [entry('notemd.md2pdf', '1.0.1', { sha256: { arm: 'local-sha' } })]
    const live = [entry('notemd.md2pdf', '1.0.1', { sha256: { arm: 'live-sha' } })]
    const r = mergeIndexes(local, live, [])
    expect(r.plugins).toHaveLength(1)
    expect(r.plugins[0].sha256).toEqual({ arm: 'local-sha' })
    expect(r.replaced).toEqual(['notemd.md2pdf@1.0.1'])
  })

  it('counts a byte-identical overlap as unchanged, not replaced', () => {
    const local = [entry('notemd.md2pdf', '1.0.1', { sha256: { arm: 'same' } })]
    const live = [entry('notemd.md2pdf', '1.0.1', { sha256: { arm: 'same' } })]
    const r = mergeIndexes(local, live, [])
    expect(r.replaced).toEqual([])
    expect(r.unchanged).toEqual(['notemd.md2pdf@1.0.1'])
  })

  it('reports local-only entries as added', () => {
    const r = mergeIndexes([entry('notemd.new', '1.0.0')], [], [])
    expect(r.added).toEqual(['notemd.new@1.0.0'])
  })

  it('drops an exact id@version from either side', () => {
    const local = [entry('notemd.pos-log', '1.0.1'), entry('notemd.pos-log', '1.1.0')]
    const live = [entry('notemd.pos-log', '1.0.2')]
    const r = mergeIndexes(local, live, ['notemd.pos-log@1.0.2'])
    const keys = r.plugins.map((p: { id: string; version: string }) => `${p.id}@${p.version}`)
    expect(keys).toEqual(['notemd.pos-log@1.0.1', 'notemd.pos-log@1.1.0'])
    expect(r.dropped).toEqual(['notemd.pos-log@1.0.2'])
  })

  it('drops every version of a bare plugin id', () => {
    const local = [entry('notemd.pos-log', '1.0.1'), entry('notemd.md2pdf', '1.0.1')]
    const live = [entry('notemd.pos-log', '1.0.3')]
    const r = mergeIndexes(local, live, ['notemd.pos-log'])
    const keys = r.plugins.map((p: { id: string; version: string }) => `${p.id}@${p.version}`)
    expect(keys).toEqual(['notemd.md2pdf@1.0.1'])
    expect(r.dropped).toEqual(['notemd.pos-log@1.0.1', 'notemd.pos-log@1.0.3'])
  })

  it('sorts the merged output by id, then version ascending', () => {
    const local = [entry('notemd.b', '1.0.10'), entry('notemd.a', '2.0.0')]
    const live = [entry('notemd.b', '1.0.9')]
    const r = mergeIndexes(local, live, [])
    const keys = r.plugins.map((p: { id: string; version: string }) => `${p.id}@${p.version}`)
    expect(keys).toEqual(['notemd.a@2.0.0', 'notemd.b@1.0.9', 'notemd.b@1.0.10'])
  })
})
