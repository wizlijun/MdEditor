import { describe, it, expect } from 'vitest'
import { isImeKey } from './ime'

/** 只造出判据用到的三个字段,不依赖任何 DOM 环境。 */
function key(props: { isComposing?: boolean; keyCode?: number; key?: string }): KeyboardEvent {
  return { isComposing: false, keyCode: 8, key: 'Backspace', ...props } as KeyboardEvent
}

describe('isImeKey', () => {
  it('isComposing 为真 = 输入法的键', () => {
    expect(isImeKey(key({ isComposing: true }))).toBe(true)
  })
  it('keyCode 229 = 输入法的键(老 webview 上 isComposing 可能缺席)', () => {
    expect(isImeKey(key({ keyCode: 229 }))).toBe(true)
  })
  it("key === 'Process' = 输入法的键", () => {
    expect(isImeKey(key({ key: 'Process', keyCode: 229 }))).toBe(true)
    expect(isImeKey(key({ key: 'Process', keyCode: 0 }))).toBe(true)
  })
  it('普通按键不受影响 —— 否则等于把编辑器的快捷键整片关掉', () => {
    expect(isImeKey(key({}))).toBe(false)
    expect(isImeKey(key({ key: 'Enter', keyCode: 13 }))).toBe(false)
    expect(isImeKey(key({ key: 'a', keyCode: 65 }))).toBe(false)
    // 229 之外的键码不该被当成输入法,包括挨着的那些
    expect(isImeKey(key({ key: 'ArrowDown', keyCode: 228 }))).toBe(false)
    expect(isImeKey(key({ key: 'ArrowDown', keyCode: 230 }))).toBe(false)
  })
  it('缺字段的事件(某些 headless 环境)按「不是输入法」处理', () => {
    expect(isImeKey({ key: 'Backspace' } as KeyboardEvent)).toBe(false)
  })
})
