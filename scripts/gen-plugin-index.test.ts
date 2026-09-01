import { describe, it, expect } from 'vitest'
import {
  applySourceMetadata,
  mergeIndexes,
  compareVersions,
  pluginCategoryFromManifest,
} from './gen-plugin-index.mjs'

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

describe('pluginCategoryFromManifest', () => {
  it('uses the first declared menu capability group', () => {
    expect(pluginCategoryFromManifest({
      contributes: { menus: [{ command: 'open', submenu: 'advance' }] },
    })).toBe('advance')
  })

  it('normalizes previous capability keys in newly generated indexes', () => {
    const aliases = {
      agents: 'advance',
      'capture-import': 'record',
      'thinking-review': 'reflect',
      'publish-export': 'create',
      'editor-extensions': 'create',
    }
    for (const [legacy, current] of Object.entries(aliases)) {
      expect(pluginCategoryFromManifest({
        contributes: { menus: [{ command: 'open', submenu: legacy }] },
      })).toBe(current)
    }
  })

  it('falls back to other for missing or unknown groups', () => {
    expect(pluginCategoryFromManifest({})).toBe('other')
    expect(pluginCategoryFromManifest({
      contributes: { menus: [{ command: 'open', submenu: 'future-category' }] },
    })).toBe('other')
  })
})

describe('applySourceMetadata', () => {
  it('refreshes official localized copy without changing immutable package fields', () => {
    const live = entry('notemd.idea-spark', '1.3.5', {
      name: 'Idea Spark',
      description: 'Old copy',
      category: 'capture',
      i18n: { zh: { name: '奇思妙想' } },
      sha256: { universal: 'keep-sha' },
      download: { universal: 'https://plugins.notemd.net/pkg' },
    })
    const [updated] = applySourceMetadata([live], [{
      id: 'notemd.idea-spark',
      name: 'Idea Spark',
      description: 'Capture a spark.',
      contributes: { menus: [{ command: 'open', submenu: 'inspiration' }] },
      i18n: { zh: { name: '奇思妙想', description: '捕捉一闪而过的灵感。' } },
    }])

    expect(updated).toMatchObject({
      version: '1.3.5',
      description: 'Capture a spark.',
      category: 'inspiration',
      i18n: { zh: { name: '奇思妙想', description: '捕捉一闪而过的灵感。' } },
      sha256: { universal: 'keep-sha' },
      download: { universal: 'https://plugins.notemd.net/pkg' },
    })
  })

  it('leaves unknown third-party entries unchanged', () => {
    const thirdParty = entry('third.party', '1.0.0', { description: 'Third party' })
    expect(applySourceMetadata([thirdParty], [])).toEqual([thirdParty])
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
