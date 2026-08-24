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
