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

  it('组内保持原有分数顺序(五种桶各自独立验证)', () => {
    // `hits` arrives already sorted by score (highest first, per
    // `searchidx::query::finish`) — grouping must not re-sort within a
    // group. Review round 1: a fixture with only `human` hits pins this for
    // 1 of N bucket kinds — a regression that re-sorts only inside
    // `derivedType`/`derivedOther`/`source`/`unlabeled` (an insert-sorted
    // push, or a `.sort()` added just in the type loop) would leave a
    // human-only fixture green. Two hits per bucket kind, interleaved in the
    // input so no bucket accidentally gets its "natural" order for free.
    // C-T10 added the `unlabeled` pair alongside the pre-existing four.
    const hits = [
      hit({ path: 'human-first.md', origin: 'human' }),
      hit({ path: 'answer-first.md', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'other-first.md', origin: 'derived', conceptType: null }),
      hit({ path: 'source-first.md', origin: 'source' }),
      hit({ path: 'unlabeled-first.md', origin: 'unlabeled' }),
      hit({ path: 'human-second.md', origin: 'human' }),
      hit({ path: 'answer-second.md', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'other-second.md', origin: 'derived', conceptType: null }),
      hit({ path: 'source-second.md', origin: 'source' }),
      hit({ path: 'unlabeled-second.md', origin: 'unlabeled' }),
    ]
    const groups = groupHits(hits)
    const byKind = (kind: string) => groups.find((g) => g.kind === kind)!.hits.map((h) => h.path)
    expect(byKind('human')).toEqual(['human-first.md', 'human-second.md'])
    expect(byKind('derivedType')).toEqual(['answer-first.md', 'answer-second.md'])
    expect(byKind('derivedOther')).toEqual(['other-first.md', 'other-second.md'])
    expect(byKind('source')).toEqual(['source-first.md', 'source-second.md'])
    expect(byKind('unlabeled')).toEqual(['unlabeled-first.md', 'unlabeled-second.md'])
  })

  it('origin unlabeled 自成一组,排在 source 之后,绝不落进 AI 产出的「其他」组(C-T10)', () => {
    // Before C-T10, `origin: 'unlabeled'` had no `HitGroupKind` of its own:
    // pre-C-T2 it fell through to `derivedOther` (rendered under the
    // AI-produced heading — a strictly stronger false claim than "raw
    // material"), and the C-T2/C-T9 interim folded it into `source` instead
    // (a different but still wrong claim: an unlabeled file might be the
    // user's own unsigned writing, not raw source material). Neither claim
    // is made now — `unlabeled` gets its own group, the fourth in the fixed
    // order, and is never conflated with `source` or `derivedOther`.
    const hits = [
      hit({ path: 'unlabeled.md', origin: 'unlabeled' }),
      hit({ path: 'src.md', origin: 'source' }),
      hit({ path: 'typed.md', origin: 'derived', conceptType: 'Answer' }),
    ]
    const groups = groupHits(hits)
    const unlabeledGroup = groups.find((g) => g.kind === 'unlabeled')
    const sourceGroup = groups.find((g) => g.kind === 'source')
    expect(unlabeledGroup?.hits.map((h) => h.path)).toEqual(['unlabeled.md'])
    expect(sourceGroup?.hits.map((h) => h.path)).toEqual(['src.md'])
    expect(groups.some((g) => g.kind === 'derivedOther' && g.hits.some((h) => h.path === 'unlabeled.md'))).toBe(false)
    // Fixed order: unlabeled is the last group, strictly after source.
    expect(groups.at(-1)?.kind).toBe('unlabeled')
    expect(groups.findIndex((g) => g.kind === 'source')).toBeLessThan(
      groups.findIndex((g) => g.kind === 'unlabeled'),
    )
  })

  it('四组顺序:你写的 → AI 产出 → 原始资料 → 未标注', () => {
    const hits = [
      hit({ path: 'h.md', origin: 'human' }),
      hit({ path: 'ans.md', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'src.md', origin: 'source' }),
      hit({ path: 'u.md', origin: 'unlabeled' }),
    ]
    const groups = groupHits(hits)
    expect(groups.map((g) => g.kind)).toEqual(['human', 'derivedType', 'source', 'unlabeled'])
  })

  it('未标注为空时不显示该组', () => {
    const hits = [hit({ origin: 'human' }), hit({ origin: 'source' })]
    const groups = groupHits(hits)
    expect(groups.some((g) => g.kind === 'unlabeled')).toBe(false)
    expect(groups.every((g) => g.hits.length > 0)).toBe(true)
  })

  it('空字符串 conceptType 与缺失一样归入「其他」', () => {
    // `row_to_hit` (searchidx/src/query.rs) deliberately keeps `NULL` and
    // `''` distinct all the way down the Rust side — its comment says that
    // distinction "matters to the grouping consumer". This is the one place
    // that consumer collapses it: `groupHits` uses a truthiness check
    // (`if (hit.conceptType)`), so `''` and `null` both fall to
    // `derivedOther`. Pinning it here keeps that Rust comment honest about
    // what actually happens on the TS side.
    const hits = [hit({ path: 'empty.md', origin: 'derived', conceptType: '' })]
    const groups = groupHits(hits)
    expect(groups).toHaveLength(1)
    expect(groups[0].kind).toBe('derivedOther')
  })
})
