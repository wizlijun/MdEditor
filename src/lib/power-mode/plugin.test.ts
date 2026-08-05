import { describe, it, expect, vi } from 'vitest'
import { createRuntime } from './plugin'
import { normalizeConfig } from './config'
import type { PowerModeConfig } from './types'

function deps() {
  return {
    shaker: { shake: vi.fn(), destroy: vi.fn() },
    combo: { hit: vi.fn(), destroy: vi.fn() },
    exploder: { fire: vi.fn(), destroy: vi.fn() },
    coords: vi.fn(() => ({ left: 11, top: 22 })),
    docSize: vi.fn(() => 100),
    docKey: vi.fn(() => 'doc-1'),
    assetBase: 'B/',
  }
}

describe('createRuntime', () => {
  const on = (over: Partial<PowerModeConfig> = {}) => normalizeConfig({ ...over })

  it('drives all three effects on a tick', () => {
    const d = deps()
    createRuntime(() => on(), d).tick()
    expect(d.shaker.shake).toHaveBeenCalledTimes(1)
    expect(d.combo.hit).toHaveBeenCalledWith(expect.anything(), 100, 'doc-1')
    expect(d.exploder.fire).toHaveBeenCalledWith(11, 22, expect.objectContaining({ size: 10 }))
  })

  it('does nothing at all when the config is null', () => {
    const d = deps()
    createRuntime(() => null, d).tick()
    expect(d.shaker.shake).not.toHaveBeenCalled()
    expect(d.combo.hit).not.toHaveBeenCalled()
    expect(d.exploder.fire).not.toHaveBeenCalled()
    expect(d.coords).not.toHaveBeenCalled()
  })

  it('gates the explosion by frequency but never the shake or combo', () => {
    const d = deps()
    // coin: frequency 4 → 第 1、5 次触发
    const rt = createRuntime(() => on({ explosion: { enable: true, presetId: 'coin' } }), d)
    for (let i = 0; i < 5; i++) rt.tick()
    expect(d.shaker.shake).toHaveBeenCalledTimes(5)
    expect(d.combo.hit).toHaveBeenCalledTimes(5)
    expect(d.exploder.fire).toHaveBeenCalledTimes(2)
  })

  it('skips each effect its own switch turns off', () => {
    const d = deps()
    createRuntime(() => normalizeConfig({
      shake: { enable: false }, combo: { enable: false }, explosion: { enable: false },
    }), d).tick()
    // shake/combo 的开关由各自模块内部判定,这里只钉「爆炸关了就不算坐标」
    expect(d.exploder.fire).not.toHaveBeenCalled()
    expect(d.coords).not.toHaveBeenCalled()
  })

  it('survives a coords lookup that throws', () => {
    const d = deps()
    d.coords.mockImplementation(() => { throw new Error('no layout') })
    expect(() => createRuntime(() => on(), d).tick()).not.toThrow()
    expect(d.shaker.shake).toHaveBeenCalledTimes(1)
  })

  it('destroy tears down every effect exactly once', () => {
    const d = deps()
    const rt = createRuntime(() => on(), d)
    rt.destroy()
    rt.destroy()
    expect(d.shaker.destroy).toHaveBeenCalledTimes(1)
    expect(d.combo.destroy).toHaveBeenCalledTimes(1)
    expect(d.exploder.destroy).toHaveBeenCalledTimes(1)
  })
})
