/**
 * 爆炸特效的宿主容器。
 *
 * 全屏 `position: fixed`,直接吃 `view.coordsAtPos()` 的视口坐标 —— 源项目把
 * div 插进编辑器容器再减 `getScrollInfo().top` 做修正,那套修正与 Obsidian
 * `coordsAtPos(pos, true)` 的 local 语义耦合,ProseMirror 没有对应模式。
 *
 * 一个窗口里可能同时有多个编辑器实例(主窗口的编辑器、Kit 实例),共用一个
 * overlay,用引用计数决定何时摘掉。
 */
const OVERLAY_CLASS = 'power-mode-overlay'

let node: HTMLElement | null = null
let holders = 0

export function acquireOverlay(root: HTMLElement = document.body): HTMLElement {
  if (!node || !node.isConnected) {
    node = document.createElement('div')
    node.className = OVERLAY_CLASS
    root.appendChild(node)
  }
  holders++
  return node
}

export function releaseOverlay(): void {
  if (holders === 0) return
  holders--
  if (holders === 0 && node) {
    node.remove()
    node = null
  }
}
