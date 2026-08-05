/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { shouldCount, comboColor, createCombo } from './combo'
import { normalizeConfig } from './config'

describe('shouldCount', () => {
  it('always counts when precisionInput is off', () => {
    expect(shouldCount(false, undefined, 10)).toBe(true)
    expect(shouldCount(false, 20, 10)).toBe(true)
  })

  it('counts only non-shrinking edits when precisionInput is on', () => {
    expect(shouldCount(true, 10, 11)).toBe(true)
    expect(shouldCount(true, 10, 10)).toBe(true)
    expect(shouldCount(true, 11, 10)).toBe(false)
  })

  it('skips the very first edit of a document (no baseline yet)', () => {
    expect(shouldCount(true, undefined, 10)).toBe(false)
  })
})

describe('comboColor', () => {
  it('walks the hue down as the streak grows', () => {
    expect(comboColor(1)).toBe('hsl(198.8, 100%, 70%)')
    expect(comboColor(10)).toBe('hsl(188, 100%, 70%)')
  })
})

describe('createCombo', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = '' })

  const cfg = (over = {}) => normalizeConfig({ combo: { timeout: 10, ...over } })

  it('renders the counter into the root and increments per hit', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    const text = document.body.querySelector('.power-mode-combo-text')!
    expect(text.textContent).toBe('1×')
    c.hit(cfg(), 2, 'doc')
    expect(text.textContent).toBe('2×')
    c.destroy()
  })

  it('hides and resets after the timeout', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    const el = document.body.querySelector('.power-mode-combo') as HTMLElement
    expect(el.style.display).toBe('flex')
    vi.advanceTimersByTime(10_000)
    expect(el.style.display).toBe('none')
    c.hit(cfg(), 2, 'doc')
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.destroy()
  })

  it('emits an exclamation every 10 hits when enabled', () => {
    const c = createCombo(document.body, () => 0)
    for (let i = 1; i <= 9; i++) c.hit(cfg(), i, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(0)
    c.hit(cfg(), 10, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(1)
    c.destroy()
  })

  it('never emits an exclamation when showExclamation is off', () => {
    const c = createCombo(document.body, () => 0)
    for (let i = 1; i <= 10; i++) c.hit(cfg({ showExclamation: false }), i, 'doc')
    expect(document.body.querySelectorAll('.power-mode-combo-exclamation')).toHaveLength(0)
    c.destroy()
  })

  it('keeps a per-document length baseline for precisionInput', () => {
    const p = cfg({ precisionInput: true })
    const c = createCombo(document.body, () => 0.9)
    c.hit(p, 100, 'a')          // 建立基线,不计数
    c.hit(p, 101, 'a')          // 变长 → 计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.hit(p, 50, 'b')           // 换文档,重新建基线,不计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.hit(p, 40, 'b')           // 变短 → 不计数
    expect(document.body.querySelector('.power-mode-combo-text')!.textContent).toBe('1×')
    c.destroy()
  })

  it('destroy removes the counter from the DOM', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(cfg(), 1, 'doc')
    c.destroy()
    expect(document.body.querySelector('.power-mode-combo')).toBeNull()
  })

  it('does nothing when combo is disabled', () => {
    const c = createCombo(document.body, () => 0.9)
    c.hit(normalizeConfig({ combo: { enable: false } }), 1, 'doc')
    expect(document.body.querySelector('.power-mode-combo')).toBeNull()
    c.destroy()
  })
})