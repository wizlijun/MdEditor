import { describe, expect, it } from 'vitest'
import type { SearchHit } from '../search/api'
import type { ResolvedSearchPlan } from './plan'
import { buildHandoffPacket, buildHandoffPrompt } from './handoff'

function hit(path: string, line = 3): SearchHit {
  return {
    path, absPath: `/vault/${path}`, line, lineEnd: line + 2, text: 'private body',
    breadcrumb: 'B', level: 'line', score: 1, docDate: null,
    sourceRef: `${path}#L${line}`, agentBy: null, humanVerified: false,
    origin: 'human', conceptType: null, pinned: false,
  }
}

describe('smart lookup handoff', () => {
  it('only carries bounded relative references and no source body', () => {
    const hits = Array.from({ length: 25 }, (_, i) => hit(`notes/${i}.md`, i + 1))
    hits.unshift(hit('../outside.md'), hit('/absolute.md'))
    const packet = buildHandoffPacket('研究延期', null, hits)
    const encoded = JSON.stringify(packet)
    expect(packet.selectedRefs).toHaveLength(20)
    expect(packet.selectedRefs.every((ref) => ref.path.startsWith('notes/'))).toBe(true)
    expect(encoded).not.toContain('private body')
    expect(encoded).not.toContain('/vault/')
    expect(new TextEncoder().encode(encoded).length).toBeLessThanOrEqual(16 * 1024)
    expect(buildHandoffPrompt(packet)).toContain('notemd search')
  })

  it('drops absolute paths and backslashes from plan-provided hints', () => {
    const plan = {
      queries: [{
        terms: ['/Users/private/vault', 'safe'], phrases: ['C:\\secret'],
        filters: { paths: ['/Users/private/vault', 'notes/project'], tags: ['safe-tag', '..\\secret'] },
      }],
      lockedFilters: {},
      sort: 'relevance',
      time: null,
    } as unknown as ResolvedSearchPlan
    const packet = buildHandoffPacket('研究延期', plan, [hit('notes/a.md')])
    const encoded = JSON.stringify(packet)
    expect(packet.queryTerms).toEqual(['safe'])
    expect(encoded).toContain('notes/project')
    expect(encoded).not.toContain('/Users/private')
    expect(encoded).not.toContain('\\\\secret')
  })

  it('keeps a worst-case Unicode packet within the byte limit', () => {
    const plan = {
      queries: [{
        terms: Array.from({ length: 24 }, (_, index) => `${'界'.repeat(255)}${index}`),
        phrases: [], filters: {},
      }],
      lockedFilters: {}, sort: 'relevance', time: null,
    } as unknown as ResolvedSearchPlan
    const packet = buildHandoffPacket('问'.repeat(2_000), plan, Array.from(
      { length: 20 }, (_, index) => hit(`notes/${index}.md`),
    ))
    expect(new TextEncoder().encode(JSON.stringify(packet)).length).toBeLessThanOrEqual(16 * 1024)
  })
})
