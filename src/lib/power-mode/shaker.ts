import type { PowerModeConfig } from './types'

/**
 * 位移量。lodash 的 `random(-i, i)` 是**整数**均匀分布,这里用连续值:视觉上
 * 没差别,而且省一个依赖。
 */
export function randomOffset(intensity: number, rnd: () => number = Math.random): { x: number; y: number } {
  const pick = () => Number(((rnd() * 2 - 1) * intensity).toFixed(4))
  return { x: pick(), y: pick() }
}

export interface Shaker {
  shake(cfg: PowerModeConfig): void
  destroy(): void
}

/**
 * 编辑区抖动。只做 CSS transform —— 整窗口物理震动(`setPosition`)是 async IPC,
 * 逐键调用掉帧,本项目明确不做。
 */
export function createShaker(el: HTMLElement, rnd: () => number = Math.random): Shaker {
  let timer: ReturnType<typeof setTimeout> | undefined

  const clear = () => {
    if (timer !== undefined) {
      clearTimeout(timer)
      timer = undefined
    }
  }

  return {
    shake(cfg) {
      if (!cfg.shake.enable) return
      clear()
      const { x, y } = randomOffset(cfg.shake.intensity, rnd)
      el.style.transform = `translate3d(${x}px, ${y}px, 0)`
      timer = setTimeout(() => {
        el.style.transform = ''
        timer = undefined
      }, cfg.shake.recoverTime)
    },
    destroy() {
      clear()
      el.style.transform = ''
    },
  }
}
