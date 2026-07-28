/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, afterEach } from 'vitest'
import { captureScroll } from './scroll-keep'

/** happy-dom 不做布局,scrollTop 是可写属性,足以验证抓取/复位语义 */
const elAt = (top: number, left = 0) => {
  const el = document.createElement('div')
  el.scrollTop = top
  el.scrollLeft = left
  return el
}

/** 复位刻意推迟到下一帧(要等 DOM 重建完),断言前先把这一帧放掉 */
const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => r()))

describe('captureScroll', () => {
  afterEach(() => vi.restoreAllMocks())

  it('restores the scroll position after the content was replaced', async () => {
    const el = elAt(320)
    const restore = captureScroll(el)
    el.scrollTop = 0            // 重载把内容换掉,浏览器把滚动清零
    restore()
    await nextFrame()
    expect(el.scrollTop).toBe(320)
  })

  it('restores horizontal scroll too', async () => {
    const el = elAt(10, 40)
    const restore = captureScroll(el)
    el.scrollTop = 0; el.scrollLeft = 0
    restore()
    await nextFrame()
    expect(el.scrollLeft).toBe(40)
  })

  it('is a no-op for a null element', () => {
    expect(() => captureScroll(null)()).not.toThrow()
    expect(() => captureScroll(undefined)()).not.toThrow()
  })

  it('does not fight the editor when we were already at the top', () => {
    // 顶部无需复位:避免与 reveal/scrollIntoView 之类的主动定位打架
    const el = elAt(0)
    const restore = captureScroll(el)
    el.scrollTop = 500          // 重载后有人主动滚到了别处
    restore()
    expect(el.scrollTop).toBe(500)
  })

  it('is idempotent — calling restore twice keeps the same position', async () => {
    const el = elAt(120)
    const restore = captureScroll(el)
    el.scrollTop = 0
    restore(); restore()
    await nextFrame()
    expect(el.scrollTop).toBe(120)
  })

  it('defers the write so it survives the re-render', () => {
    const raf = vi.spyOn(globalThis, 'requestAnimationFrame')
    const el = elAt(50)
    captureScroll(el)()
    expect(raf).toHaveBeenCalled()
  })
})
