import { describe, it, expect } from 'vitest'
import { groupHits } from './grouping'
import type { SearchHit } from './api'

// Minimal, fully-typed `SearchHit` factory — `groupHits` only reads `origin`
// and `conceptType`, everything else is filler so the object satisfies the
// interface.
function hit(overrides: Partial<SearchHit>): SearchHit {
  return {
    path: 'a.md',
    absPath: '/v/a.md',
    line: 1,
    lineEnd: 1,
    text: 'x',
    breadcrumb: '',
    level: 'line',
    score: 0.5,
    docDate: null,
    sourceRef: 'a.md#L1',
    agentBy: null,
    humanVerified: false,
    origin: 'derived',
    conceptType: null,
    ...overrides,
  }
}

describe('groupHits', () => {
  it('两极固定在首尾,中间按类型', () => {
    const hits = [
      hit({ path: 'src.md', origin: 'source' }),
      hit({ path: 'ans.md', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'human.md', origin: 'human' }),
    ]
    const groups = groupHits(hits)
    expect(groups[0].kind).toBe('human')
    expect(groups[groups.length - 1].kind).toBe('source')
    // The derived type group must sit strictly between the two poles.
    expect(groups.slice(1, -1).every((g) => g.kind === 'derivedType' || g.kind === 'derivedOther')).toBe(true)
  })

  it('空组不显示', () => {
    // No `source` hit anywhere in the results — the "原始资料" group must not
    // appear at all, not appear empty.
    const hits = [hit({ origin: 'human' }), hit({ origin: 'derived', conceptType: 'Answer' })]
    const groups = groupHits(hits)
    expect(groups.some((g) => g.kind === 'source')).toBe(false)
    expect(groups.every((g) => g.hits.length > 0)).toBe(true)
  })

  it('组数随结果中出现的类型数变化(2 种类型 → 4 组)', () => {
    const hits = [
      hit({ path: 'h.md', origin: 'human' }),
      hit({ path: 'bs.md', origin: 'derived', conceptType: 'Book Summary' }),
      hit({ path: 'ans.md', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'src.md', origin: 'source' }),
    ]
    const groups = groupHits(hits)
    expect(groups.length).toBe(4)
    expect(groups.map((g) => g.kind)).toEqual(['human', 'derivedType', 'derivedType', 'source'])

    // A third type present in the results grows the group count again —
    // pins that the count truly tracks the data, not a fixed constant.
    const withThirdType = [...hits, hit({ path: 'idea.md', origin: 'derived', conceptType: 'Idea Proof' })]
    expect(groupHits(withThirdType).length).toBe(5)
  })

  it('derived 里没有类型的归入「其他」并排在具名类型之后', () => {
    const hits = [
      hit({ path: 'untyped.md', origin: 'derived', conceptType: null }),
      hit({ path: 'typed.md', origin: 'derived', conceptType: 'Answer' }),
    ]
    const groups = groupHits(hits)
    const otherIndex = groups.findIndex((g) => g.kind === 'derivedOther')
    const namedIndex = groups.findIndex((g) => g.kind === 'derivedType')
    expect(otherIndex).toBeGreaterThan(-1)
    expect(namedIndex).toBeGreaterThan(-1)
    expect(otherIndex).toBeGreaterThan(namedIndex)
  })

  it('组内保持原有分数顺序', () => {
    // `hits` arrives already sorted by score (highest first, per
    // `searchidx::query::finish`) — grouping must not re-sort within a group.
    const hits = [
      hit({ path: 'first.md', origin: 'human', score: 0.9 }),
      hit({ path: 'second.md', origin: 'human', score: 0.4 }),
      hit({ path: 'third.md', origin: 'human', score: 0.1 }),
    ]
    const groups = groupHits(hits)
    expect(groups[0].hits.map((h) => h.path)).toEqual(['first.md', 'second.md', 'third.md'])
  })
})
