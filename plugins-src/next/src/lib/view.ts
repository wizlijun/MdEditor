import { itemSearchText, type WorkspaceItem } from './repository'
import { normalizeDue, normalizePriority, PRIORITIES } from './metadata'

export type SortMode = 'priority' | 'due' | 'created'

type ItemComparator = (left: WorkspaceItem, right: WorkspaceItem) => number

function comparePriority(left: WorkspaceItem, right: WorkspaceItem): number {
  return PRIORITIES.indexOf(normalizePriority(left.priority ?? left.task?.priority))
    - PRIORITIES.indexOf(normalizePriority(right.priority ?? right.task?.priority))
}

function compareDue(left: WorkspaceItem, right: WorkspaceItem): number {
  const leftDue = normalizeDue(left.due ?? left.task?.due)
  const rightDue = normalizeDue(right.due ?? right.task?.due)
  if (!leftDue && !rightDue) return 0
  if (!leftDue) return 1
  if (!rightDue) return -1
  return leftDue.localeCompare(rightDue)
}

function createdAt(item: WorkspaceItem): number | undefined {
  for (const value of [item.created, item.projection?.last_at]) {
    if (!value) continue
    const timestamp = Date.parse(value)
    if (!Number.isNaN(timestamp)) return timestamp
  }
  return undefined
}

function compareCreated(left: WorkspaceItem, right: WorkspaceItem): number {
  const leftCreated = createdAt(left)
  const rightCreated = createdAt(right)
  if (leftCreated === undefined && rightCreated === undefined) return 0
  if (leftCreated === undefined) return 1
  if (rightCreated === undefined) return -1
  return rightCreated - leftCreated
}

const sortChains: Record<SortMode, ItemComparator[]> = {
  priority: [comparePriority, compareDue, compareCreated],
  due: [compareDue, comparePriority, compareCreated],
  created: [compareCreated, comparePriority, compareDue],
}

/** Sort a copy for board display without changing repository or lifecycle order. */
export function sortWorkspaceItems(items: readonly WorkspaceItem[], mode: SortMode): WorkspaceItem[] {
  const chain = sortChains[mode]
  return items.slice().sort((left, right) => {
    for (const compare of chain) {
      const result = compare(left, right)
      if (result) return result
    }
    return left.key.localeCompare(right.key)
  })
}

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
