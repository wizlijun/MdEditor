/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { randomOffset, createShaker } from './shaker'
import { normalizeConfig } from './config'

describe('randomOffset', () => {
  it('maps rnd() 0 / 0.5 / 1 onto -i / 0 / +i', () => {
    expect(randomOffset(5, () => 0)).toEqual({ x: -5, y: -5 })
    expect(randomOffset(5, () => 0.5)).toEqual({ x: 0, y: 0 })
    expect(randomOffset(4, () => 1)).toEqual({ x: 4, y: 4 })
  })
})

describe('createShaker', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('translates on shake and clears after recoverTime', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({ shake: { enable: true, intensity: 3, recoverTime: 800 } }))
    expect(el.style.transform).toBe('translate3d(3px, 3px, 0)')
    vi.advanceTimersByTime(799)
    expect(el.style.transform).toBe('translate3d(3px, 3px, 0)')
    vi.advanceTimersByTime(1)
    expect(el.style.transform).toBe('')
    s.destroy()
  })

  it('does nothing when shake is disabled', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({ shake: { enable: false } }))
    expect(el.style.transform).toBe('')
    s.destroy()
  })

  it('destroy clears the pending recovery and resets the transform', () => {
    const el = document.createElement('div')
    const s = createShaker(el, () => 1)
    s.shake(normalizeConfig({ shake: { enable: true } }))
    s.destroy()
    expect(el.style.transform).toBe('')
    // 定时器已被取消:再走完也不该抛
    vi.advanceTimersByTime(5000)
  })
})