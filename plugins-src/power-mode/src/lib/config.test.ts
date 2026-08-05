import { describe, it, expect } from 'vitest'
import { DEFAULT_CONFIG, normalizeConfig, isSurfaceEnabled, PRESET_IDS } from './config'
// 主程序的原件。运行时不可 import(隔离 webview),但测试跑在 node 里,可以拿它
// 当漂移哨兵。
import { DEFAULT_CONFIG as HOST_DEFAULT } from '../../../../src/lib/power-mode/config'

describe('parity with the host copy', () => {
  it('keeps DEFAULT_CONFIG identical', () => {
    expect(DEFAULT_CONFIG).toEqual(HOST_DEFAULT)
  })
})

describe('config', () => {
  it('lists exactly the four shipped presets', () => {
    expect([...PRESET_IDS].sort()).toEqual(['coin', 'confetti', 'lightning', 'particle'])
  })

  it('normalizes junk to the defaults', () => {
    expect(normalizeConfig(null)).toEqual(DEFAULT_CONFIG)
  })

  it('defaults main off and plugin surfaces on', () => {
    const cfg = normalizeConfig({})
    expect(isSurfaceEnabled(cfg, 'main')).toBe(false)
    expect(isSurfaceEnabled(cfg, 'notemd.whatever')).toBe(true)
  })
})
