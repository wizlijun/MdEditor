import { itemSearchText, type WorkspaceItem } from './repository'

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
