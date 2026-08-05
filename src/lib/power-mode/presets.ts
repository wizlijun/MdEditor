import type { ExplosionConfig, PresetId } from './types'

export type PresetParams = Omit<ExplosionConfig, 'imageList'> & { frameCount: number }

/**
 * 四个预设的参数,逐字来自 ~/git/obsidian-power-mode/src/presets/explosion/。
 * 素材路径不在这里:见 `presetConfig`。
 */
export const PRESET_PARAMS: Record<PresetId, PresetParams> = {
  particle: {
    maxExplosions: 3, size: 10, frequency: 1, explosionOrder: 'random',
    gifMode: 'continue', duration: 400, offset: 0.25, backgroundMode: 'mask', frameCount: 8,
  },
  lightning: {
    maxExplosions: 15, size: 15, frequency: 2, explosionOrder: 'sequential',
    gifMode: 'restart', duration: 1000, offset: 0.2, backgroundMode: 'image', frameCount: 3,
    customStyle: { mixBlendMode: 'color-dodge' },
  },
  coin: {
    maxExplosions: 5, size: 8, frequency: 4, explosionOrder: 'random',
    gifMode: 'restart', duration: 1500, offset: 0.66, backgroundMode: 'image', frameCount: 1,
  },
  confetti: {
    maxExplosions: 5, size: 26, frequency: 3, explosionOrder: 'random',
    gifMode: 'restart', duration: 1200, offset: 0.32, backgroundMode: 'image', frameCount: 1,
  },
}

/**
 * 素材根 URL。
 *
 * 必须相对**本模块自己的 URL** 解析,不能用 `import x from './a.gif'`:
 * - 主窗口:本模块在 `/assets/<chunk>.js` → `/assets/power-mode/`
 * - 插件窗口的 Editor Kit:在 `plugin://<id>/__host__/assets/editor-kit-v1.js`
 *   → `plugin://<id>/__host__/assets/power-mode/`(而 `__host__` 只镜像
 *   `dist/assets/`,所以这条路径正好命中)
 *
 * Vite 注入的绝对路径 `/assets/…` 在插件窗口里会解析成
 * `plugin://<id>/assets/…`(插件自己的 ui/ 目录)→ 404。
 *
 * dev 分支:主窗口由 Vite dev server 服务,本模块的 URL 是
 * `/src/lib/power-mode/presets.ts`,相对解析会指错;publicDir 在 dev 下挂在根,
 * 所以直接写绝对路径。built 出来的 Kit 里 `import.meta.env.DEV` 是 false,
 * 两条分支不会互相干扰。
 */
export function assetBase(): string {
  if (import.meta.env.DEV) return '/assets/power-mode/'
  return new URL(/* @vite-ignore */ './power-mode/', import.meta.url).href
}

/** 预设参数 + 素材路径 = 可直接喂给 exploder 的配置。 */
export function presetConfig(id: PresetId, base: string = assetBase()): ExplosionConfig {
  const { frameCount, ...rest } = PRESET_PARAMS[id]
  return {
    ...rest,
    imageList: Array.from({ length: frameCount }, (_, i) => `${base}${id}/${i + 1}.gif`),
  }
}
