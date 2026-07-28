// src/lib/scroll-keep.ts
// 重载后把滚动位置留在原处。外部改动通常发生在你视线之外的位置,像素级复位就够用
// (块锚定式恢复要引入身份匹配,性价比不划算)。

/**
 * 抓取当前滚动位置,返回一个「重载完成后调用即复位」的闭包。
 *
 * 复位放在 requestAnimationFrame 里:必须等 DOM 重建完成,否则刚写进去的
 * scrollTop 会被随后的渲染清掉。闭包幂等,重复调用无副作用;元素为空返回 no-op。
 */
export function captureScroll(el: HTMLElement | null | undefined): () => void {
  if (!el) return () => {}
  const top = el.scrollTop
  const left = el.scrollLeft
  if (top === 0 && left === 0) return () => {}   // 本就在顶部,无需复位
  return () => {
    const apply = () => { el.scrollTop = top; el.scrollLeft = left }
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(apply)
    else apply()
  }
}
