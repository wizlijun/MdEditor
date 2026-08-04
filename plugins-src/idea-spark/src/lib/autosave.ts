export const AUTOSAVE_MS = 1500

export interface Autosave {
  /** 内容变了:重排定时器。 */
  schedule(): void
  /** 立刻写盘(切换/关窗/委托/Cmd+S),并取消待触发的定时器。 */
  flush(): Promise<void>
  /** 拆除:取消定时器,不写盘。 */
  dispose(): void
}

/**
 * 停笔 `delayMs` 后写盘。`save` 抛错只被吞掉(调用方自己把失败反映到 UI),
 * 但不能让后续的 schedule 失效 —— 磁盘临时写不进去不该让自动保存从此罢工。
 *
 * 并发约束:任意时刻至多一次 `save()` 在飞(`inFlight`)。定时器触发或 `flush()`
 * 发现已有一次在飞时,不会另起一次,而是等它结束后再按当时的 `pending` 决定
 * 是否补发一次 —— 避免旧内容的 save 晚于新内容 resolve、静默覆盖磁盘。
 */
export function createAutosave(save: () => Promise<void>, delayMs = AUTOSAVE_MS): Autosave {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending = false
  let inFlight: Promise<void> | null = null
  let disposed = false

  // 若已有 save 在飞,等它结束;结束后若 pending 仍为真(可能是等待期间新排的),
  // 补发一次并等它结束。任意时刻只会有一个 save() 调用在飞。
  const settle = async () => {
    if (inFlight) {
      await inFlight
    }
    if (!pending) return
    pending = false
    const p = (async () => {
      try { await save() } catch { /* 调用方负责显示失败 */ }
    })()
    inFlight = p
    try {
      await p
    } finally {
      if (inFlight === p) inFlight = null
    }
  }

  return {
    schedule() {
      if (disposed) return
      pending = true
      if (timer != null) clearTimeout(timer)
      timer = setTimeout(() => {
        timer = null
        void settle()
      }, delayMs)
    },
    async flush() {
      if (disposed) return
      if (timer != null) { clearTimeout(timer); timer = null }
      await settle()
    },
    dispose() {
      disposed = true
      if (timer != null) { clearTimeout(timer); timer = null }
      pending = false
    },
  }
}
