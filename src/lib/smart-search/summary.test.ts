import { describe, expect, it } from 'vitest'
import { validateSummaryOutput, type SummarySource } from './summary'

const sources: SummarySource[] = [
  { id: 'S1', path: 'a.md', line: 1, lineEnd: 2 },
  { id: 'S2', path: 'b.md', line: 4, lineEnd: 4 },
]

describe('quick summary validation', () => {
  it('accepts at most three cited bullets', () => {
    expect(validateSummaryOutput('- 延期源于依赖。[S1]\n- 风险仍待确认。[S2]', sources, 'bullets'))
      .toMatchObject({ citations: ['S1', 'S2'] })
  })

  it('rejects unknown, missing and excessive citations', () => {
    expect(() => validateSummaryOutput('没有引用', sources, 'sentence')).toThrow('缺少')
    expect(() => validateSummaryOutput('未知。[S9]', sources, 'sentence')).toThrow('未知')
    expect(() => validateSummaryOutput('- a [S1]\n- b [S1]\n- c [S2]\n- d [S2]', sources, 'bullets'))
      .toThrow('三个')
    expect(() => validateSummaryOutput('第一段。[S1]\n第二段。[S2]', sources, 'sentence'))
      .toThrow('一个段落')
  })
})
