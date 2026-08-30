import { itemSearchText, type WorkspaceItem } from './repository'

export interface PreviewAnchorRect {
  left: number
  right: number
  top: number
}

export interface PreviewSize {
  width: number
  height: number
}

/** Keep a portaled preview beside its card and wholly inside the viewport. */
export function previewPosition(
  anchor: PreviewAnchorRect,
  tip: PreviewSize,
  viewport: PreviewSize,
  margin = 12,
  gap = 10,
): { x: number; y: number } {
  const right = anchor.right + gap
  const left = anchor.left - tip.width - gap
  const fitsRight = right + tip.width <= viewport.width - margin
  const fitsLeft = left >= margin
  const rightSpace = viewport.width - anchor.right - gap
  const leftSpace = anchor.left - gap
  const preferredX = fitsRight
    ? right
    : fitsLeft
      ? left
      : rightSpace >= leftSpace ? right : left
  const maxX = Math.max(margin, viewport.width - tip.width - margin)
  const maxY = Math.max(margin, viewport.height - tip.height - margin)
  return {
    x: Math.max(margin, Math.min(preferredX, maxX)),
    y: Math.max(margin, Math.min(anchor.top, maxY)),
  }
}

export function isDormantDue(item: WorkspaceItem, today = new Date()): boolean {
  if (item.projection?.state !== 'dormant') return false
  const match = item.projection.wake_trigger.match(/^(\d{4}-\d{2}-\d{2})(?:\b|T)/)
  if (!match) return false
  const due = new Date(`${match[1]}T00:00:00`)
  return !Number.isNaN(due.getTime()) && due.getTime() <= today.getTime()
}

export function placedItems(items: WorkspaceItem[], query: string): WorkspaceItem[] {
  const normalized = query.trim().toLocaleLowerCase()
  return items
    .filter((item) => (
      item.state === 'dormant'
      || item.state === 'closed'
      || item.state === 'unsupported'
      || (item.state === 'capture' && item.orphan)
    ))
    .filter((item) => !normalized || itemSearchText(item).includes(normalized))
}
