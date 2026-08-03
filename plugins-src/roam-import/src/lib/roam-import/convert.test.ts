import { describe, it, expect } from 'vitest'
import { convertPage } from './convert'
// @ts-expect-error - 宿主仓库的纯 JS lint core(OKF 硬约束校验)
import { lintText } from '../../../../../scripts/okf-lint-core.mjs'

const page = {
  title: '某个概念',
  uid: 'abc123',
  'create-time': 1_600_000_000_000,
  'edit-time': 1_700_000_000_000,
  children: [{ uid: 'b1', string: '一条内容' }],
}

describe('convertPage — OKF frontmatter', () => {
  it('stamps the concept type so the imported page satisfies the hard constraints', () => {
    const out = convertPage(page, new Set(), new Map())
    expect(out.text).toContain('type: Outline Note')
    expect(lintText('某个概念.note.md', out.text)).toEqual([])
  })

  it('keeps the daily page type distinct', () => {
    const daily = { ...page, title: 'August 15th, 2022', uid: '08-15-2022' }
    const out = convertPage(daily, new Set(), new Map())
    expect(out.text).toContain('type: Daily Note')
    expect(lintText('2022-08-15.note.md', out.text)).toEqual([])
  })
})
