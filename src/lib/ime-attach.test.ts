// @vitest-environment happy-dom
//
// `attachImeGuard` 的 DOM 行为。单独一个文件,是因为它需要真实的事件派发路径
// —— 这里要钉住的正是「谁先收到这一下按键」,而那只有在真的 dispatch 一次
// 事件时才成立。ime.test.ts 那边是纯函数,留在 node 环境里跑。
import { describe, it, expect } from 'vitest'
import { attachImeGuard } from './ime'

describe('attachImeGuard — 必须从祖先捕获,才抢得过 ProseMirror', () => {
  /** 模拟 PM:监听器挂在 contenteditable 自己身上,且比我们先注册。 */
  function stage() {
    document.body.innerHTML = ''
    const host = document.createElement('div')
    const pm = document.createElement('div')
    pm.contentEditable = 'true'
    host.appendChild(pm)
    document.body.appendChild(host)
    const seen: string[] = []
    pm.addEventListener('keydown', (e) => seen.push((e as KeyboardEvent).key))   // 先注册
    const detach = attachImeGuard(host)                                          // 后注册
    return { host, pm, seen, detach }
  }

  function press(el: HTMLElement, key: string) {
    const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
    el.dispatchEvent(ev)
    return ev
  }

  it('收尾那一击被取消:PM 的监听器根本收不到,原生行为也被 preventDefault', () => {
    const { pm, seen, detach } = stage()
    pm.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    pm.dispatchEvent(new Event('compositionend', { bubbles: true }))

    const ev = press(pm, 'Backspace')

    expect(seen, 'PM 收到了就说明祖先捕获没起作用').toEqual([])
    expect(ev.defaultPrevented, '不 preventDefault 的话 contenteditable 会自己删一个字').toBe(true)
    detach()
  })

  it('没变换过的按键原样放行 —— 否则等于把键盘整个吃掉', () => {
    const { pm, seen, detach } = stage()
    const ev = press(pm, 'Backspace')
    expect(seen).toEqual(['Backspace'])
    expect(ev.defaultPrevented).toBe(false)
    detach()
  })

  it('只吃收尾那一下,紧接着的第二下照常放行', () => {
    const { pm, seen, detach } = stage()
    pm.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    pm.dispatchEvent(new Event('compositionend', { bubbles: true }))
    press(pm, 'Backspace')
    press(pm, 'Backspace')
    expect(seen, '连吃两下就成了「退格失灵」').toEqual(['Backspace'])
    detach()
  })

  it('detach 之后彻底不插手', () => {
    const { pm, seen, detach } = stage()
    detach()
    pm.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    pm.dispatchEvent(new Event('compositionend', { bubbles: true }))
    const ev = press(pm, 'Backspace')
    expect(seen).toEqual(['Backspace'])
    expect(ev.defaultPrevented).toBe(false)
    detach()
  })
})
