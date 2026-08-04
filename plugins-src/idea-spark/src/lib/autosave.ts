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
 */
export function createAutosave(save: () => Promise<void>, delayMs = AUTOSAVE_MS): Autosave {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending = false
  const run = async () => {
    timer = null
    if (!pending) return
    pending = false
    try { await save() } catch { /* 调用方负责显示失败 */ }
  }
  return {
    schedule() {
      pending = true
      if (timer != null) clearTimeout(timer)
      timer = setTimeout(() => void run(), delayMs)
    },
    async flush() {
      if (timer != null) { clearTimeout(timer); timer = null }
      await run()
    },
    dispose() {
      if (timer != null) { clearTimeout(timer); timer = null }
      pending = false
    },
  }
}
