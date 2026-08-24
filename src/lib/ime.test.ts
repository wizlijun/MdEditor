import { describe, it, expect } from 'vitest'
import { isImeKey, createImeGuard } from './ime'

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

describe('createImeGuard — 结束变换的那一下按键', () => {
  /** 手动推进的时钟：尾巴窗口是时间判据，不能靠真实时间碰运气。 */
  function guardAt() {
    let t = 1000
    const g = createImeGuard(() => t)
    return { g, tick: (ms: number) => { t += ms } }
  }

  it('变换进行中的按键一律拦下', () => {
    const { g } = guardAt()
    g.start()
    expect(g.blocks(key({}))).toBe(true)
  })

  it('规范顺序：keydown(isComposing) 在 compositionend 之前 —— 拦下', () => {
    const { g } = guardAt()
    g.start()
    expect(g.blocks(key({ isComposing: true }))).toBe(true)
    g.end()
  })

  it('WebKit 的反序：compositionend 先来，随后那一下 keydown 什么标记都没有 —— 仍要拦下', () => {
    const { g, tick } = guardAt()
    g.start()
    g.end()          // 删掉最后一个预编辑字符，变换先结束
    tick(1)          // 同一次按键的事件序列，紧挨着
    expect(g.blocks(key({}))).toBe(true)
  })

  it('尾巴过去之后键盘还给编辑器 —— 否则等于把退格锁死', () => {
    const { g, tick } = guardAt()
    g.start()
    g.end()
    tick(200)        // 人再按下一次的间隔
    expect(g.blocks(key({}))).toBe(false)
  })

  it('从没变换过时不拦任何键', () => {
    const { g } = guardAt()
    expect(g.blocks(key({}))).toBe(false)
    expect(g.blocks(key({ key: 'Enter', keyCode: 13 }))).toBe(false)
  })

  it('reset 立刻交还键盘（失焦后不该留下卡住的状态）', () => {
    const { g } = guardAt()
    g.start()
    g.reset()
    expect(g.blocks(key({}))).toBe(false)
  })
})

describe('createImeGuard.consumeTail — contenteditable 要主动取消，不能只是「不管」', () => {
  function guardAt() {
    let t = 1000
    const g = createImeGuard(() => t)
    return { g, tick: (ms: number) => { t += ms } }
  }

  it('收尾那一击报 true —— 调用方据此 preventDefault', () => {
    const { g, tick } = guardAt()
    g.start(); g.end(); tick(1)
    expect(g.consumeTail(key({}))).toBe(true)
  })

  it('只报一次:第二下是用户真的又按了一次,不能连着吃', () => {
    const { g, tick } = guardAt()
    g.start(); g.end(); tick(1)
    expect(g.consumeTail(key({}))).toBe(true)
    expect(g.consumeTail(key({})), '连吃两下就成了「退格失灵」').toBe(false)
  })

  it('变换还在进行中不算收尾 —— 那些键要原样交给输入法,取消掉会打断变换', () => {
    const { g } = guardAt()
    g.start()
    expect(g.consumeTail(key({ isComposing: true }))).toBe(false)
  })

  it('尾巴之外一律不取消', () => {
    const { g, tick } = guardAt()
    g.start(); g.end(); tick(200)
    expect(g.consumeTail(key({}))).toBe(false)
  })

  it('从没变换过时不取消任何键', () => {
    const { g } = guardAt()
    expect(g.consumeTail(key({}))).toBe(false)
  })
})
