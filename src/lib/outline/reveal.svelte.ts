export interface RevealRequest {
  seq: number
  /** 主文档 1-based 行号 */
  line: number
  /** 该行的锚文本（标题文本/高亮文本），rich 模式与 debounce 窗口兜底搜索用 */
  text: string
  /**
   * 目标文件的绝对路径。给的是「这条请求属于哪个文档」——消费方（RichEditor /
   * SourceView）用它判断该不该认领。
   *
   * 之所以需要:EditorPane 用 `{#key tab.id}` 包住编辑器,换文件 = 编辑器整个
   * 重建。搜索面板是先 `openFile` 再发请求,新编辑器实例挂载时请求已经在了,
   * 因此消费方必须能认领「早于自己挂载」的请求;而一旦能认领旧请求,就必须能
   * 分辨它是不是发给自己的,否则切到别的 tab 会把上一次的定位套到新文档上。
   *
   * 省略时任何文档都认领 —— 大纲面板在同一文档内跳转,不需要这层保护。
   */
  path?: string | null
  /** Zero-based top-level heading index for precise TOC navigation in rich mode. */
  headingIndex?: number
}

export const reveal = $state<{ req: RevealRequest | null }>({ req: null })

let seq = 0
export function requestReveal(
  line: number,
  text: string,
  path?: string | null,
  options: Pick<RevealRequest, 'headingIndex'> = {},
): void {
  reveal.req = { seq: ++seq, line, text, path, ...options }
}
