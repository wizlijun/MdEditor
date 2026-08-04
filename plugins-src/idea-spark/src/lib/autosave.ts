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
 *
 * `flush()` 的契约是**返回时磁盘已经追上当时的内容**,而且这条契约对*每一个*
 * 并发的等待者都成立。所以 `settle` 是个循环而不是「等一次 + 看一眼 pending」:
 * 两个 flush 同时等同一次在飞的保存时,先醒的那个会消费掉 `pending` 并补发一次,
 * 后醒的若只看 `pending`(此刻已是 false)就会**不等补发直接放行** —— 调用方
 * (App 的 `keepUnsaved`)据此判定「已经存好了」而换掉编辑器内容,补发那次随后
 * 才落地并把旧文档的 `current`/`savedMarkdown` 写回 store,新草稿于是继承了旧
 * idea 的文件名并覆盖它。循环到「既无 pending 也无 in-flight」才返回,堵的就是
 * 这条路。
 */
export function createAutosave(save: () => Promise<void>, delayMs = AUTOSAVE_MS): Autosave {
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending = false
  let inFlight: Promise<void> | null = null
  let disposed = false

  // 有在飞的就等它;没有在飞但有 pending 就补发一次并等它。反复直到两者皆无 ——
  // 于是无论多少个 flush 并发等待,每一个都只在「最后一次 save 已落地」之后返回,
  // 哪怕补发那次是别的等待者发起的。任意时刻仍只有一个 save() 在飞。
  const settle = async () => {
    while (inFlight || pending) {
      if (inFlight) {
        await inFlight
        continue
      }
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
