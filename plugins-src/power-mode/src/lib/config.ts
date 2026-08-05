// ⚠️ 本文件是 src/lib/power-mode/config.ts 的裁剪副本(去掉 resolveExplosion 与
// presets 依赖:插件不渲染特效,只编辑配置)。改动请与主程序同步 ——
// config.test.ts 的 parity 用例会在 DEFAULT_CONFIG 漂移时报警。
import type { ExplosionConfig, PowerModeConfig, PresetId } from './types'

/** 与主程序 PRESET_PARAMS 的键集一致;这里只需要 id,不需要参数。 */
export const PRESET_IDS: readonly PresetId[] = ['particle', 'lightning', 'coin', 'confetti']

/**
 * 出厂默认。
 *
 * `surfaces.main` 默认关、插件窗口默认开:狂暴模式在主编辑窗口是干扰,在
 * 「随手写一条」的插件窗口里才是那点仪式感。
 *
 * 震动默认关(最易分心);intensity/recoverTime 取「中度」档、timeout 取「中」档。
 */
export const DEFAULT_CONFIG: PowerModeConfig = {
  surfaces: { main: false, 'notemd.idea-spark': true },
  shake: { enable: false, intensity: 6, recoverTime: 800 },
  combo: { enable: true, timeout: 10, showExclamation: true, precisionInput: false },
  explosion: { enable: true, presetId: 'particle' },
}

function obj(v: unknown): Record<string, unknown> {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {}
}
function bool(v: unknown, fallback: boolean): boolean {
  return typeof v === 'boolean' ? v : fallback
}
function num(v: unknown, fallback: number): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback
}

/** 把 RPC 上来的任意 JSON 收敛成一份完整配置。永不抛。 */
export function normalizeConfig(raw: unknown): PowerModeConfig {
  const r = obj(raw)
  const shake = obj(r.shake)
  const combo = obj(r.combo)
  const explosion = obj(r.explosion)
  const presetId = explosion.presetId
  const overrides = r.overrides && typeof r.overrides === 'object' && !Array.isArray(r.overrides)
    ? (r.overrides as Partial<ExplosionConfig>)
    : undefined
  const surfaces: Record<string, boolean> = { ...DEFAULT_CONFIG.surfaces }
  for (const [k, v] of Object.entries(obj(r.surfaces))) {
    if (typeof v === 'boolean') surfaces[k] = v
  }
  return {
    surfaces,
    shake: {
      enable: bool(shake.enable, DEFAULT_CONFIG.shake.enable),
      intensity: num(shake.intensity, DEFAULT_CONFIG.shake.intensity),
      recoverTime: num(shake.recoverTime, DEFAULT_CONFIG.shake.recoverTime),
    },
    combo: {
      enable: bool(combo.enable, DEFAULT_CONFIG.combo.enable),
      timeout: num(combo.timeout, DEFAULT_CONFIG.combo.timeout),
      showExclamation: bool(combo.showExclamation, DEFAULT_CONFIG.combo.showExclamation),
      precisionInput: bool(combo.precisionInput, DEFAULT_CONFIG.combo.precisionInput),
    },
    explosion: {
      enable: bool(explosion.enable, DEFAULT_CONFIG.explosion.enable),
      presetId: (typeof presetId === 'string' && (PRESET_IDS as readonly string[]).includes(presetId))
        ? (presetId as PresetId)
        : DEFAULT_CONFIG.explosion.presetId,
    },
    ...(overrides ? { overrides } : {}),
  }
}

/**
 * 某个生效面是否开着。未列出的插件窗口默认开,未列出的 'main' 默认关。
 * (插件 UI 用它给 checkbox 算初始勾选态。)
 */
export function isSurfaceEnabled(cfg: PowerModeConfig | null, surfaceId: string): boolean {
  if (!cfg) return false
  const explicit = cfg.surfaces[surfaceId]
  if (typeof explicit === 'boolean') return explicit
  return surfaceId !== 'main'
}
