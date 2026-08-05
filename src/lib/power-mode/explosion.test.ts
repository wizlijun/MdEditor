/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { pickImage, restartUrl, createExploder } from './explosion'
import { presetConfig } from './presets'
import type { ExplosionConfig } from './types'

describe('pickImage', () => {
  const list = ['a', 'b', 'c']

  it('random picks by rnd()', () => {
    expect(pickImage(list, 'random', 0, () => 0)).toBe('a')
    expect(pickImage(list, 'random', 0, () => 0.99)).toBe('c')
  })

  it('sequential walks the list by the tick count', () => {
    expect(pickImage(list, 'sequential', 0)).toBe('a')
    expect(pickImage(list, 'sequential', 4)).toBe('b')
  })

  it('a numeric order pins one frame, out-of-range falls back to the first', () => {
    expect(pickImage(list, 1, 7)).toBe('b')
    expect(pickImage(list, 9, 7)).toBe('a')
  })
})

describe('restartUrl', () => {
  it('appends a cache-busting stamp with the right separator', () => {
    expect(restartUrl('http://x/a.gif', 42)).toBe('http://x/a.gif?t=42')
    expect(restartUrl('http://x/a.gif?v=1', 42)).toBe('http://x/a.gif?v=1&t=42')
  })
})

const unquote = (v: string) => v.replace(/"/g, '')

describe('createExploder', () => {
  let overlay: HTMLElement
  beforeEach(() => {
    vi.useFakeTimers()
    overlay = document.createElement('div')
    document.body.appendChild(overlay)
  })
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = '' })

  const cfg = (over: Partial<ExplosionConfig> = {}): ExplosionConfig =>
    ({ ...presetConfig('coin', 'B/'), ...over })

  it('places a layer at the given viewport coordinates', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(100, 50, cfg())
    const el = overlay.querySelector('.power-mode-explosion') as HTMLElement
    expect(el).not.toBeNull()
    expect(el.style.left).toBe('100px')
    expect(el.style.top).toBe('50px')
    expect(el.style.width).toBe('8ch')
    expect(el.style.height).toBe('8rem')
    expect(el.style.marginTop).toBe('-5.28rem')   // -offset(0.66) * size(8)
    e.destroy()
  })

  it('uses backgroundImage in image mode and maskImage in mask mode', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ backgroundMode: 'image', gifMode: 'continue', imageList: ['B/x.gif'] }))
    const img = overlay.querySelector('.power-mode-explosion') as HTMLElement
    expect(img.classList.contains('power-mode-explosion-image')).toBe(true)
    // 引号由 CSSOM 决定(happy-dom 规范化 backgroundImage 却不碰 maskImage),
    // 断言对它不敏感。
    expect(unquote(img.style.backgroundImage)).toBe('url(B/x.gif)')

    e.fire(0, 0, cfg({ backgroundMode: 'mask', gifMode: 'continue', imageList: ['B/y.gif'] }))
    const mask = overlay.querySelectorAll('.power-mode-explosion')[1] as HTMLElement
    expect(mask.classList.contains('power-mode-explosion-mask')).toBe(true)
    expect(unquote(mask.style.maskImage)).toBe('url(B/y.gif)')
    e.destroy()
  })

  it('stamps the url in restart mode only', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ gifMode: 'restart', imageList: ['B/x.gif'] }))
    expect(unquote((overlay.firstElementChild as HTMLElement).style.backgroundImage)).toBe('url(B/x.gif?t=7)')
    e.destroy()
  })

  it('applies customStyle', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ customStyle: { mixBlendMode: 'color-dodge' } }))
    const el = overlay.firstElementChild as HTMLElement
    expect(el.style.getPropertyValue('mix-blend-mode')).toBe('color-dodge')
    e.destroy()
  })

  it('caps the live layers at maxExplosions, dropping the oldest', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    for (let i = 0; i < 5; i++) e.fire(i, 0, cfg({ maxExplosions: 2, duration: 10_000 }))
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(2)
    e.destroy()
  })

  it('removes a layer when its duration expires, freeing its slot', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    // 上游 bug:index 为 0 (最旧那个) 时不从数组里摘,陈旧条目占着
    // maxExplosions 名额,后来的活跃特效被提前裁掉。这里钉死修正后的行为。
    e.fire(0, 0, cfg({ maxExplosions: 3, duration: 100 }))
    vi.advanceTimersByTime(100)
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(0)
    for (let i = 0; i < 3; i++) e.fire(i, 0, cfg({ maxExplosions: 3, duration: 10_000 }))
    expect(overlay.querySelectorAll('.power-mode-explosion')).toHaveLength(3)
    e.destroy()
  })

  it('does nothing when the image list is empty', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ imageList: [] }))
    expect(overlay.children).toHaveLength(0)
    e.destroy()
  })

  it('destroy clears every live layer and its timer', () => {
    const e = createExploder(overlay, () => 0, () => 7)
    e.fire(0, 0, cfg({ duration: 10_000 }))
    e.destroy()
    expect(overlay.children).toHaveLength(0)
    vi.advanceTimersByTime(20_000)
  })
})
