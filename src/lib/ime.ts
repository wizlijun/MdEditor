/**
 * 一次按键到底是谁的?
 *
 * 非英语输入法在候选窗口开着时,键盘先归输入法:退格删的是预编辑串里的一个
 * 字符,回车确认的是候选项,方向键翻的是候选列表。这段时间浏览器**照样**派发
 * `keydown` —— 于是任何不加判断就处理 Backspace / Enter / 方向键的编辑器,会
 * 把同一下按键处理第二遍:输入法删一个字符,应用再删一个(或另起一行、跳到别
 * 的节点)。用户看到的就是「多删了一个字」。
 *
 * 判据按可靠性排序:
 *
 *  1. `isComposing` —— 规范里就是为这件事定义的,`compositionstart` 之后、
 *     `compositionend` 之前的 keydown 都为 true。WebKit/Chromium 都实现了。
 *  2. `keyCode === 229` —— Chromium 与 Android 把所有交给输入法处理的键统一
 *     报成 229;老 webview 上 `isComposing` 不一定在,229 一定在。229 不是任何
 *     实体键的码,不会误伤。
 *  3. `key === 'Process'` —— 规范给「这一下被输入法吃掉了」定义的键名,
 *     Chromium/Firefox 在部分平台上发它。
 *
 * 三条是「或」的关系:任何一条成立,这一下就不归我们。
 *
 * 已有的参照实现是搜索面板(`SearchPanel.svelte` 的 `onKeydown`):
 * 「关掉候选窗口的那个回车属于输入法,不属于我们」。这里只是把同一条判断
 * 变成所有编辑器共用的一处。
 */
export function isImeKey(e: KeyboardEvent): boolean {
  return e.isComposing === true || e.keyCode === 229 || e.key === 'Process'
}

/**
 * 结束这一段变换的**那一下**按键,`isImeKey` 是抓不住的。
 *
 * 删预编辑串的最后一个字符时,这一下同时做两件事:删掉字符、结束变换。规范里
 * 它应该是 `keydown(isComposing=true)` → `compositionend`;但 WebKit 系的
 * webview 会反过来,先 `compositionend`、再派发 `keydown`,而那时
 * `isComposing` 已经是 false 了。于是「一个一个删,删到最后一个」正好落在
 * 唯一一个漏判的位置上 —— 用户看到的是最后一下退格把前面的内容也吃掉了。
 *
 * 所以判据不能只看单个事件,得看这一段时间:`compositionend` 之后极短的一个
 * 尾巴内到达的按键,仍然算这一段变换的收尾。一次物理按键的事件序列不会跨越
 * 这么长,而人也打不了这么快 —— 鼠标点选候选项之后再按键同理,隔着一次移动
 * 和一次点击,不可能落在窗口内。
 */
const TAIL_MS = 60

export interface ImeGuard {
  /** compositionstart */
  start(): void
  /** compositionend */
  end(): void
  /** 失焦等场合彻底复位,免得状态卡住把键盘锁死 */
  reset(): void
  /** true = 这一下按键归输入法,别处理 */
  blocks(e: KeyboardEvent): boolean
  /**
   * true = 这一下就是**收尾**那一击(变换已结束、按键刚到)。**只报一次**,
   * 报过就把窗口关掉。
   *
   * 和 `blocks` 分开,是因为两种输入区的处置正相反:
   *
   *  - `<textarea>`:输入法已经把该做的做完了,我们「不管」就对了 —— `blocks`。
   *  - contenteditable(ProseMirror):PM 自己也认得这一击(`inOrNearComposition`,
   *    prosemirror-view/dist/index.js),但它只是 `return`,**不** `preventDefault`。
   *    于是 contenteditable 的原生退格照跑,吃掉一个已确定的字符。这里必须主动
   *    把这一击**取消**掉,「不管」是不够的。
   *
   * 一次性,是为了不误伤鼠标点选候选之后的第一次真按键。
   */
  consumeTail(e: KeyboardEvent): boolean
}

/** `now` 可注入,纯粹为了测试能确定性地推进时间。 */
export function createImeGuard(now: () => number = () => Date.now()): ImeGuard {
  let composing = false
  let endedAt = -Infinity
  return {
    start() { composing = true },
    end() { composing = false; endedAt = now() },
    reset() { composing = false; endedAt = -Infinity },
    blocks(e: KeyboardEvent) {
      return composing || isImeKey(e) || now() - endedAt < TAIL_MS
    },
    consumeTail(_e: KeyboardEvent) {
      if (composing || now() - endedAt >= TAIL_MS) return false
      endedAt = -Infinity
      return true
    },
  }
}

/**
 * 给一块 contenteditable(ProseMirror)装上收尾守卫,返回卸载函数。
 *
 * **必须挂在祖先上,而且是捕获阶段。** ProseMirror 的键盘监听器挂在
 * `.ProseMirror` 元素自己身上,而按键的 target 正是那个 contenteditable ——
 * 同一元素上 at-target 阶段不分捕获/冒泡,一律按注册顺序触发,PM 又挂得比
 * 我们早。所以「在 .ProseMirror 上 addEventListener(..., true)」抢不到它前面,
 * 只有从祖先捕获才行。
 *
 * @param host `.ProseMirror` 的祖先容器
 * @param guard 复用调用方已有的守卫(它可能还要用 `blocks`);默认自建一个
 */
export function attachImeGuard(host: HTMLElement, guard: ImeGuard = createImeGuard()): () => void {
  const onStart = () => guard.start()
  const onEnd = () => guard.end()
  const onKeydown = (e: Event) => {
    if (!guard.consumeTail(e as KeyboardEvent)) return
    e.preventDefault()
    e.stopImmediatePropagation()
  }
  host.addEventListener('compositionstart', onStart, true)
  host.addEventListener('compositionend', onEnd, true)
  host.addEventListener('keydown', onKeydown, true)
  return () => {
    host.removeEventListener('compositionstart', onStart, true)
    host.removeEventListener('compositionend', onEnd, true)
    host.removeEventListener('keydown', onKeydown, true)
  }
}

/**
 * 把 Rich 编辑器实例和它的 IME 守卫绑成同一个生命周期。
 *
 * Rich 挂载函数应直接返回这个包装后的实例，而不是要求每个上层组件记得另挂
 * `attachImeGuard`。这样无论主窗口、Editor Kit，还是以后新增的 Rich 消费方，
 * 都天然拥有同一套保护；调用编辑器原有的 `destroy()` 时也会自动卸载监听器。
 */
export function guardRichEditor<T extends { destroy(): void }>(
  host: HTMLElement,
  instance: T,
  guard: ImeGuard = createImeGuard(),
): T {
  const detach = attachImeGuard(host, guard)
  const destroy = instance.destroy.bind(instance)
  let active = true
  instance.destroy = () => {
    if (!active) return
    active = false
    detach()
    destroy()
  }
  return instance
}
