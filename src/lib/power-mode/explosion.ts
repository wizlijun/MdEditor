import type { ExplosionConfig } from './types'

/** 按 explosionOrder 选一帧。`count` 是本引擎实例迄今的触发次数。 */
export function pickImage(
  list: string[],
  order: ExplosionConfig['explosionOrder'],
  count: number,
  rnd: () => number = Math.random,
): string {
  if (order === 'random') return list[Math.floor(rnd() * list.length)] ?? list[0]
  if (order === 'sequential') return list[count % list.length]
  return list[order] ?? list[0]
}

/**
 * 让 GIF 从头播:加一个时间戳查询参数,换一个新的资源 URL。
 *
 * 源项目对 base64 是往字符串里插 `t=…;base64,`(一个 hack);本项目素材是外部
 * 文件,查询参数就够了。
 */
export function restartUrl(url: string, ts: number): string {
  return `${url}${url.includes('?') ? '&' : '?'}t=${ts}`
}

/** `gifMode: 'continue'` 依赖素材已在缓存里,否则第一次触发只看得到白框。 */
export function preloadFrames(list: string[]): void {
  if (typeof Image === 'undefined') return
  for (const src of list) {
    const img = new Image()
    img.src = src
  }
}

interface Live {
  el: HTMLElement
  clock: ReturnType<typeof setTimeout>
}

export interface Exploder {
  /** `left`/`top` 是视口坐标,直接来自 `view.coordsAtPos()`。 */
  fire(left: number, top: number, cfg: ExplosionConfig): void
  destroy(): void
}

export function createExploder(
  overlay: HTMLElement,
  rnd: () => number = Math.random,
  now: () => number = Date.now,
): Exploder {
  let count = -1
  const active: Live[] = []
  const preloaded = new Set<string>()

  function drop(entry: Live): void {
    const i = active.indexOf(entry)
    // 上游写的是 `if (index > 0)`,最旧那个(index 0)永远摘不掉,陈旧条目
    // 占着 maxExplosions 名额。用 indexOf + splice 直接表达正确语义。
    if (i >= 0) active.splice(i, 1)
    entry.el.remove()
    clearTimeout(entry.clock)
  }

  return {
    fire(left, top, cfg) {
      count++
      if (cfg.imageList.length === 0) return

      if (cfg.gifMode === 'continue') {
        const cold = cfg.imageList.filter((u) => !preloaded.has(u))
        if (cold.length) {
          preloadFrames(cold)
          for (const u of cold) preloaded.add(u)
        }
      }

      const el = document.createElement('div')
      el.classList.add('power-mode-explosion', `power-mode-explosion-${cfg.backgroundMode}`)
      el.style.left = `${left}px`
      el.style.top = `${top}px`
      el.style.width = `${cfg.size}ch`
      el.style.height = `${cfg.size}rem`
      el.style.marginTop = `${-(cfg.offset || 0) * cfg.size}rem`

      let url = pickImage(cfg.imageList, cfg.explosionOrder, count, rnd)
      if (cfg.gifMode === 'restart') url = restartUrl(url, now())
      if (cfg.backgroundMode === 'image') {
        el.style.backgroundImage = `url(${url})`
      } else {
        el.style.webkitMaskImage = `url(${url})`
        el.style.maskImage = `url(${url})`
      }
      for (const [k, v] of Object.entries(cfg.customStyle ?? {})) {
        el.style.setProperty(k.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`), v)
      }

      overlay.appendChild(el)
      const entry: Live = { el, clock: setTimeout(() => drop(entry), cfg.duration) }
      active.push(entry)

      while (cfg.maxExplosions > 0 && active.length > cfg.maxExplosions) {
        const oldest = active[0]
        if (!oldest) break
        drop(oldest)
      }
    },
    destroy() {
      while (active.length) drop(active[0]!)
      preloaded.clear()
    },
  }
}
