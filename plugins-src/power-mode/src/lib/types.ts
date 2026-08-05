// ⚠️ 本文件是 src/lib/power-mode/types.ts 的副本。插件跑在隔离 webview 里,
// 不能 import 主程序的 src/(见 docs/plugin-v2-development.md §2)。
// 改动请两边同步 —— config.test.ts 的 parity 用例会在漂移时报警。

// Power Mode 的数据形状。移植自 ~/git/obsidian-power-mode 的 type.d.ts,
// 去掉了 shakeWindow / useCustom / customEffect(本项目明确不做)。

export type PresetId = 'particle' | 'lightning' | 'coin' | 'confetti'

export interface ExplosionConfig {
  /** 同屏最大特效数,超出则移除最旧的。 */
  maxExplosions: number
  /** 宽 = size ch,高 = size rem。依赖字体度量,预设值需按本项目字体调。 */
  size: number
  /** 每 N 次输入触发一次。 */
  frequency: number
  explosionOrder: 'random' | 'sequential' | number
  /** 'restart' = 每次重放 GIF(加 ?t= 时间戳);'continue' = 复用浏览器缓存里正在播的那帧。 */
  gifMode: 'continue' | 'restart'
  /** 特效存活毫秒。 */
  duration: number
  /** 上移 offset × size rem。 */
  offset: number
  /** 'mask' = 用 currentColor 填充 + mask-image(自动跟随主题文字色)。 */
  backgroundMode: 'mask' | 'image'
  imageList: string[]
  /** 直接抹到 element.style 上的额外样式(lightning 用它上 mix-blend-mode)。 */
  customStyle?: Record<string, string>
}

export interface PowerModeConfig {
  /**
   * 每个生效面一个开关。key 是 'main'(主编辑窗口)或插件 id。
   * 缺省语义见 `isSurfaceEnabled`:'main' 默认关,插件窗口默认开。
   */
  surfaces: Record<string, boolean>
  shake: { enable: boolean; intensity: number; recoverTime: number }
  combo: { enable: boolean; timeout: number; showExclamation: boolean; precisionInput: boolean }
  explosion: { enable: boolean; presetId: PresetId }
  /** 用户在内置预设之上的改动。内置预设本身只存 id,参数从代码常量读。 */
  overrides?: Partial<ExplosionConfig>
}
