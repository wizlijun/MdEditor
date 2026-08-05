import { Plugin, PluginKey } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import type { PowerModeConfig } from './types'
import { resolveExplosion } from './config'
import { assetBase } from './presets'
import { createShaker, type Shaker } from './shaker'
import { createCombo, type Combo } from './combo'
import { createExploder, type Exploder } from './explosion'
import { acquireOverlay, releaseOverlay } from './overlay'

export type ConfigGetter = () => PowerModeConfig | null

export const powerModeKey = new PluginKey('powerMode')

export interface RuntimeDeps {
  shaker: Pick<Shaker, 'shake' | 'destroy'>
  combo: Pick<Combo, 'hit' | 'destroy'>
  exploder: Pick<Exploder, 'fire' | 'destroy'>
  /** 光标在视口里的位置。抛异常时静默跳过爆炸(布局还没稳时会抛)。 */
  coords: () => { left: number; top: number }
  docSize: () => number
  docKey: () => string
  assetBase: string
}

export interface PowerModeRuntime {
  tick(): void
  destroy(): void
}

/**
 * 一次文档变更 = 一次 tick。
 *
 * 计数器是**每实例**的:源项目用模块级 `count`,主窗口 + 若干 Kit 实例并存时
 * frequency 门控会互相串。
 */
export function createRuntime(getConfig: ConfigGetter, deps: RuntimeDeps): PowerModeRuntime {
  let count = -1
  let dead = false

  return {
    tick() {
      if (dead) return
      const cfg = getConfig()
      if (!cfg) return
      count++
      deps.shaker.shake(cfg)
      deps.combo.hit(cfg, deps.docSize(), deps.docKey())
      if (!cfg.explosion.enable) return
      const explosion = resolveExplosion(cfg, deps.assetBase)
      if (count % Math.max(1, explosion.frequency) !== 0) return
      try {
        const { left, top } = deps.coords()
        deps.exploder.fire(left, top, explosion)
      } catch {
        // coordsAtPos 在布局尚未成型时会抛;跳过这一发,别把输入链路带崩。
      }
    },
    destroy() {
      if (dead) return
      dead = true
      deps.shaker.destroy()
      deps.combo.destroy()
      deps.exploder.destroy()
    },
  }
}

/**
 * 把引擎接到一个 ProseMirror 编辑器上。
 *
 * `getConfig` 返回 null = 这个生效面关着(判定在调用方,引擎不认识生效面)。
 * `docKey` 用于 precisionInput 的每文档长度基线;主窗口传文件路径,Kit 传实例 id。
 */
export function powerModePlugin(getConfig: ConfigGetter, docKey: () => string = () => 'default'): Plugin {
  return new Plugin({
    key: powerModeKey,
    view(view: EditorView) {
      const overlay = acquireOverlay()
      const rt = createRuntime(getConfig, {
        shaker: createShaker(view.dom.parentElement ?? view.dom),
        combo: createCombo(document.body),
        exploder: createExploder(overlay),
        coords: () => {
          const c = view.coordsAtPos(view.state.selection.head)
          return { left: c.left, top: c.top }
        },
        docSize: () => view.state.doc.content.size,
        docKey,
        assetBase: assetBase(),
      })
      return {
        update(v, prevState) {
          if (v.state.doc.eq(prevState.doc)) return
          rt.tick()
        },
        destroy() {
          rt.destroy()
          releaseOverlay()
        },
      }
    },
  })
}
