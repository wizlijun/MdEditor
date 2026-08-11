import { describe, it, expect } from 'vitest'
import {
  decideTrigger,
  looksLikeAWord,
  BOUNDARY_DELAY_MS,
  IDLE_DELAY_MS,
} from './input-trigger'

describe('decideTrigger', () => {
  // 中文输入法在选词前会把拼音串当普通输入丢进 input 事件。那串字符不是用户
  // 想搜的东西,而且几乎必然 FTS 未命中 —— 正是最贵的那条路径。
  it('never searches while an IME composition is in progress', () => {
    expect(decideTrigger('sousuo', true)).toEqual({ kind: 'hold' })
    expect(decideTrigger('搜索', true)).toEqual({ kind: 'hold' })
  })

  it('fires fast once a word boundary is typed', () => {
    expect(decideTrigger('增量索引 ', false)).toEqual({ kind: 'search', delayMs: BOUNDARY_DELAY_MS })
    expect(decideTrigger('alpha,', false)).toEqual({ kind: 'search', delayMs: BOUNDARY_DELAY_MS })
    expect(decideTrigger('这一段。', false)).toEqual({ kind: 'search', delayMs: BOUNDARY_DELAY_MS })
  })

  // `"` 开短语、`:` 开过滤器(tag:/path:),在这套查询语言里它们是词中字符。
  it('does not treat the query language’s own syntax as a word boundary', () => {
    expect(decideTrigger('tag:', false)).toEqual({ kind: 'search', delayMs: IDLE_DELAY_MS })
    expect(decideTrigger('"exact', false)).toEqual({ kind: 'search', delayMs: IDLE_DELAY_MS })
  })

  it('waits out a pause mid-word', () => {
    expect(decideTrigger('增量', false)).toEqual({ kind: 'search', delayMs: IDLE_DELAY_MS })
    expect(decideTrigger('alpha', false)).toEqual({ kind: 'search', delayMs: IDLE_DELAY_MS })
  })

  it('clears on an empty query instead of searching for nothing', () => {
    expect(decideTrigger('', false)).toEqual({ kind: 'clear' })
    expect(decideTrigger('   ', false)).toEqual({ kind: 'clear' })
  })

  it('holds a single latin character but searches a single CJK character', () => {
    expect(decideTrigger('a', false)).toEqual({ kind: 'hold' })
    expect(decideTrigger('增', false)).toEqual({ kind: 'search', delayMs: IDLE_DELAY_MS })
    expect(looksLikeAWord('a')).toBe(false)
    expect(looksLikeAWord('あ')).toBe(true)
    expect(looksLikeAWord('한')).toBe(true)
  })
})
