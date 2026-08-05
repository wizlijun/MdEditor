import type { PowerModeConfig } from './types'

/** 移植自源项目 combo.ts。刻意不做 i18n:这是游戏音效性质的彩蛋,不是 UI 文案。 */
export const EXCLAMATIONS: readonly string[] = [
  'Super!', 'Fantastic!', 'Great!', 'OMG', 'Whoah!', ':O', 'Nice!',
  'Splendid!', 'Grand!', 'Impressive!', 'Stupendous!', 'Extreme!', 'Awesome!',
]

/**
 * 这一次编辑是否该记进连击。
 *
 * `precisionInput` 打开时只认「文档没变短」的编辑(删除不算连击)。文档第一次
 * 被编辑时还没有基线,源项目在这种情况下不计数 —— 保持一致。
 */
export function shouldCount(precisionInput: boolean, prev: number | undefined, cur: number): boolean {
  if (!precisionInput) return true
  return prev !== undefined && prev <= cur
}

/** 连击色:连得越久越偏青。 */
export function comboColor(count: number): string {
  return `hsl(${200 - count * 1.2}, 100%, 70%)`
}

export interface Combo {
  hit(cfg: PowerModeConfig, docSize: number, docKey: string): void
  destroy(): void
}

/**
 * 右上角连击计数器。
 *
 * 每个引擎实例一份 —— 源项目用的是模块级单例,主窗口 + 若干 Kit 窗口同时存在
 * 时会互相串号。
 *
 * 所有 `animate()` 都写成可选调用:jsdom 没有 Web Animations API。
 */
export function createCombo(root: HTMLElement, rnd: () => number = Math.random): Combo {
  let count = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let flickerTimer: ReturnType<typeof setTimeout> | undefined
  let el: HTMLElement | undefined
  let textEl: HTMLElement
  let progressEl: HTMLElement
  const lengthMap = new Map<string, number>()

  function ensure(): void {
    if (el) return
    el = document.createElement('div')
    el.className = 'power-mode-combo'
    textEl = document.createElement('div')
    textEl.className = 'power-mode-combo-text'
    progressEl = document.createElement('div')
    progressEl.className = 'power-mode-combo-progress'
    el.append(textEl, progressEl)
    root.appendChild(el)
  }

  function flickAnimate(target: HTMLElement): void {
    target.animate?.(
      [
        { opacity: 1, filter: 'invert(0)' },
        { opacity: 0.3, filter: 'invert(0.6)' },
        { opacity: 1, filter: 'invert(0)' },
      ],
      { duration: 30 },
    )
  }

  function flicker(): void {
    flickAnimate(progressEl)
    if (rnd() < 0.5) flickAnimate(textEl)
    flickerTimer = setTimeout(flicker, 100 + rnd() * 700)
  }

  function stopFlicker(): void {
    if (flickerTimer !== undefined) {
      clearTimeout(flickerTimer)
      flickerTimer = undefined
    }
  }

  function reset(): void {
    if (timer !== undefined) {
      clearTimeout(timer)
      timer = undefined
    }
    stopFlicker()
    count = 0
    if (el) el.style.display = 'none'
  }

  function exclaim(color: string): void {
    const node = document.createElement('div')
    node.className = 'power-mode-combo-exclamation'
    node.textContent = EXCLAMATIONS[Math.floor(rnd() * EXCLAMATIONS.length)] ?? EXCLAMATIONS[0]
    node.style.color = color
    el!.appendChild(node)
    node.animate?.(
      [
        { transform: 'translate3d(0,0,0)', opacity: 1 },
        { transform: `translate3d(${Math.round((rnd() * 2 - 1) * 20)}%, 200%, 0)`, opacity: 0 },
      ],
      { duration: 2000 },
    )
    setTimeout(() => node.remove(), 2000)
  }

  function active(cfg: PowerModeConfig): void {
    count++
    if (count === 1) flicker()
    el!.style.display = 'flex'
    const color = comboColor(count)
    textEl.style.textShadow = `0 0 15px ${color}, 0 1px ${color}, 1px 0 ${color}, 0 -1px ${color}, -1px 0 ${color}`
    textEl.textContent = `${count}×`
    progressEl.style.boxShadow = `0 0 15px ${color}`
    progressEl.style.borderColor = color
    progressEl.style.width = `${count * 10}%`
    progressEl.animate?.([{ width: '80px' }, { width: '0px' }], { duration: cfg.combo.timeout * 1000 })
    textEl.animate?.([{ transform: 'scale(1.5)' }, { transform: 'scale(1)' }], { duration: 150 })
    if (cfg.combo.showExclamation && count % 10 === 0) exclaim(color)
    if (timer !== undefined) clearTimeout(timer)
    timer = setTimeout(reset, cfg.combo.timeout * 1000)
  }

  return {
    hit(cfg, docSize, docKey) {
      if (!cfg.combo.enable) return
      if (shouldCount(cfg.combo.precisionInput, lengthMap.get(docKey), docSize)) {
        ensure()
        active(cfg)
      }
      lengthMap.set(docKey, docSize)
    },
    destroy() {
      reset()
      el?.remove()
      el = undefined
      lengthMap.clear()
    },
  }
}
