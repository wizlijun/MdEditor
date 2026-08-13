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
    pinned: false,
    ...overrides,
  }
}

/** Every hit of a group, in render order — the shape the older assertions in
 *  this file were written against, before hits were bucketed by file. */
function allHits(group: { files: { hits: SearchHit[] }[] }): SearchHit[] {
  return group.files.flatMap((f) => f.hits)
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

  // --- 置顶组(wikipage 检索优先级 spec §4)---------------------------------

  it('置顶命中单独成组、排在最前', () => {
    const groups = groupHits([
      hit({ path: 'wikipage/张三.md', origin: 'derived', pinned: true }),
      hit({ path: 'human.md', origin: 'human' }),
    ])
    expect(groups[0].kind).toBe('pinned')
    expect(allHits(groups[0]).map((h) => h.path)).toEqual(['wikipage/张三.md'])
  })

  it('置顶命中不再出现在它本来的 origin 组里', () => {
    // 后端已经把它排到了第一位;若这里还按 origin 分一次组,它就会沉到
    // 中间某个组里去,「第一条」在视觉上直接失效 —— 这正是要分出置顶组的
    // 理由,所以必须断言它没有被重复渲染。
    const groups = groupHits([hit({ path: 'p.md', origin: 'human', pinned: true })])
    expect(groups.map((g) => g.kind)).toEqual(['pinned'])
  })

  it('没有置顶命中时不渲染空的置顶组', () => {
    const groups = groupHits([hit({ path: 'a.md', origin: 'human' })])
    expect(groups.some((g) => g.kind === 'pinned')).toBe(false)
  })

  it('空组不显示', () => {
    // No `source` hit anywhere in the results — the "原始资料" group must not
    // appear at all, not appear empty.
    const hits = [hit({ origin: 'human' }), hit({ origin: 'derived', conceptType: 'Answer' })]
    const groups = groupHits(hits)
    expect(groups.some((g) => g.kind === 'source')).toBe(false)
    expect(groups.every((g) => g.hitCount > 0)).toBe(true)
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
    const byKind = (kind: string) => allHits(groups.find((g) => g.kind === kind)!).map((h) => h.path)
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
    expect(allHits(unlabeledGroup!).map((h) => h.path)).toEqual(['unlabeled.md'])
    expect(allHits(sourceGroup!).map((h) => h.path)).toEqual(['src.md'])
    expect(
      groups.some((g) => g.kind === 'derivedOther' && allHits(g).some((h) => h.path === 'unlabeled.md')),
    ).toBe(false)
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
    expect(groups.every((g) => g.hitCount > 0)).toBe(true)
  })

  it('未标注组同样按文件折叠 —— 折叠层不能只覆盖前四组', () => {
    // The file-folding layer (2026-08-12 readability work) and the fifth
    // `unlabeled` group (C-T10) landed on the two sides of a merge. Both
    // features are built from the same `makeGroup` helper, so folding is
    // structurally uniform — but nothing else in this file exercises the
    // combination, and "take one side wholesale" is exactly how one of them
    // would have been lost. Pinned here so a regression that special-cases
    // `unlabeled` back onto a flat `hits` array fails loudly.
    const hits = [
      hit({ path: 'raw/a.txt', line: 3, origin: 'unlabeled' }),
      hit({ path: 'raw/b.txt', line: 1, origin: 'unlabeled' }),
      hit({ path: 'raw/a.txt', line: 12, origin: 'unlabeled' }),
    ]
    const group = groupHits(hits).find((g) => g.kind === 'unlabeled')!
    expect(group.files.map((f) => f.path)).toEqual(['raw/a.txt', 'raw/b.txt'])
    expect(group.files[0].hits.map((h) => h.line)).toEqual([3, 12])
    expect(group.files[0].name).toBe('a.txt')
    expect(group.hitCount).toBe(3)
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

  it('同文件的多条命中聚成一个文件条目', () => {
    const hits = [
      hit({ path: 'notes/a.md', line: 3, origin: 'human' }),
      hit({ path: 'notes/b.md', line: 9, origin: 'human' }),
      hit({ path: 'notes/a.md', line: 41, origin: 'human' }),
    ]
    const files = groupHits(hits)[0].files
    expect(files.map((f) => f.path)).toEqual(['notes/a.md', 'notes/b.md'])
    expect(files[0].hits.map((h) => h.line)).toEqual([3, 41])
    expect(groupHits(hits)[0].hitCount).toBe(3)
  })

  it('文件顺序取首次出现,即其最高分命中的位置', () => {
    // `hits` arrives score-sorted, so "first appearance" is "best hit" — a
    // file whose only hit ranks last must not jump ahead of one that opened
    // the list.
    const hits = [
      hit({ path: 'best.md', origin: 'human' }),
      hit({ path: 'worst.md', origin: 'human' }),
      hit({ path: 'best.md', origin: 'human' }),
      hit({ path: 'best.md', origin: 'human' }),
    ]
    expect(groupHits(hits)[0].files.map((f) => f.path)).toEqual(['best.md', 'worst.md'])
  })

  it('name 是 basename,path/absPath 原样带出', () => {
    const hits = [hit({ path: 'a/b/会议纪要.md', absPath: '/v/a/b/会议纪要.md', origin: 'human' })]
    expect(groupHits(hits)[0].files[0]).toMatchObject({
      path: 'a/b/会议纪要.md',
      absPath: '/v/a/b/会议纪要.md',
      name: '会议纪要.md',
    })
  })

  it('同名文件跨类型组不合并 —— 它确实同时占着两极', () => {
    const hits = [
      hit({ path: 'both.md', line: 1, origin: 'human' }),
      hit({ path: 'both.md', line: 2, origin: 'source' }),
    ]
    const groups = groupHits(hits)
    expect(groups.map((g) => g.kind)).toEqual(['human', 'source'])
    expect(groups[0].files.map((f) => f.path)).toEqual(['both.md'])
    expect(groups[1].files.map((f) => f.path)).toEqual(['both.md'])
    expect(groups[0].files[0].hits).toHaveLength(1)
  })
})
