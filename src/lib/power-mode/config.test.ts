import { describe, it, expect } from 'vitest'
import { PRESET_PARAMS, presetConfig } from './presets'
import { DEFAULT_CONFIG, normalizeConfig, isSurfaceEnabled, resolveExplosion } from './config'

describe('presets', () => {
  it('carries the four upstream presets verbatim', () => {
    expect(Object.keys(PRESET_PARAMS).sort()).toEqual(['coin', 'confetti', 'lightning', 'particle'])
    expect(PRESET_PARAMS.particle).toMatchObject({
      maxExplosions: 3, size: 10, frequency: 1, explosionOrder: 'random',
      gifMode: 'continue', duration: 400, offset: 0.25, backgroundMode: 'mask', frameCount: 8,
    })
    expect(PRESET_PARAMS.lightning).toMatchObject({
      maxExplosions: 15, size: 15, frequency: 2, explosionOrder: 'sequential',
      gifMode: 'restart', duration: 1000, offset: 0.2, backgroundMode: 'image', frameCount: 3,
    })
    expect(PRESET_PARAMS.lightning.customStyle).toEqual({ mixBlendMode: 'color-dodge' })
    expect(PRESET_PARAMS.coin).toMatchObject({ maxExplosions: 5, size: 8, frequency: 4, duration: 1500, offset: 0.66, frameCount: 1 })
    expect(PRESET_PARAMS.confetti).toMatchObject({ maxExplosions: 5, size: 26, frequency: 3, duration: 1200, offset: 0.32, frameCount: 1 })
  })

  it('builds 1-based frame urls under the given base', () => {
    const cfg = presetConfig('particle', 'https://host/assets/power-mode/')
    expect(cfg.imageList).toHaveLength(8)
    expect(cfg.imageList[0]).toBe('https://host/assets/power-mode/particle/1.gif')
    expect(cfg.imageList[7]).toBe('https://host/assets/power-mode/particle/8.gif')
    // frameCount 是拼装用的,不该漏进运行时配置
    expect('frameCount' in cfg).toBe(false)
  })
})

describe('normalizeConfig', () => {
  it('fills every branch from defaults when given junk', () => {
    for (const junk of [null, undefined, 42, 'x', []]) {
      expect(normalizeConfig(junk)).toEqual(DEFAULT_CONFIG)
    }
  })

  it('deep-merges partial input without dropping sibling keys', () => {
    const out = normalizeConfig({ combo: { timeout: 3 }, explosion: { presetId: 'coin' } })
    expect(out.combo).toEqual({ enable: true, timeout: 3, showExclamation: true, precisionInput: false })
    expect(out.explosion).toEqual({ enable: true, presetId: 'coin' })
    expect(out.shake).toEqual(DEFAULT_CONFIG.shake)
  })

  it('rejects an unknown presetId and falls back to the default', () => {
    expect(normalizeConfig({ explosion: { presetId: 'pikachu' } }).explosion.presetId).toBe('particle')
  })

  it('keeps user surface flags and merges them over the defaults', () => {
    const out = normalizeConfig({ surfaces: { main: true } })
    expect(out.surfaces.main).toBe(true)
    expect(out.surfaces['notemd.idea-spark']).toBe(true)
  })

  it('drops a non-object overrides but keeps a real one', () => {
    expect(normalizeConfig({ overrides: 'nope' }).overrides).toBeUndefined()
    expect(normalizeConfig({ overrides: { size: 20 } }).overrides).toEqual({ size: 20 })
  })
})

describe('isSurfaceEnabled', () => {
  it('is false for every surface when the config is null', () => {
    expect(isSurfaceEnabled(null, 'main')).toBe(false)
    expect(isSurfaceEnabled(null, 'notemd.idea-spark')).toBe(false)
  })

  it('defaults main off and any unknown plugin surface on', () => {
    const cfg = normalizeConfig({ surfaces: {} })
    expect(isSurfaceEnabled(cfg, 'main')).toBe(false)
    expect(isSurfaceEnabled(cfg, 'notemd.somebody-new')).toBe(true)
  })

  it('honours an explicit flag either way', () => {
    const cfg = normalizeConfig({ surfaces: { main: true, 'notemd.idea-spark': false } })
    expect(isSurfaceEnabled(cfg, 'main')).toBe(true)
    expect(isSurfaceEnabled(cfg, 'notemd.idea-spark')).toBe(false)
  })
})

describe('resolveExplosion', () => {
  it('returns the preset when there are no overrides', () => {
    const cfg = normalizeConfig({ explosion: { presetId: 'coin' } })
    expect(resolveExplosion(cfg, 'B/')).toEqual(presetConfig('coin', 'B/'))
  })

  it('lets overrides win over the preset', () => {
    const cfg = normalizeConfig({ explosion: { presetId: 'coin' }, overrides: { size: 42, frequency: 1 } })
    const out = resolveExplosion(cfg, 'B/')
    expect(out.size).toBe(42)
    expect(out.frequency).toBe(1)
    expect(out.duration).toBe(1500) // 预设值原样保留
    expect(out.imageList).toEqual(presetConfig('coin', 'B/').imageList)
  })
})
