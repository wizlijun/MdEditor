import type { SearchHit } from '../search/api'

export function hitKey(hit: Pick<SearchHit, 'path' | 'line' | 'lineEnd'>): string {
  return `${hit.path}\0${hit.line}\0${hit.lineEnd}`
}

export interface SelectionModifiers {
  toggle: boolean
  range: boolean
}

/** Standard desktop-list selection. The caller owns focus and the range anchor. */
export function chooseResultKeys(
  ordered: string[],
  current: string[],
  key: string,
  anchor: string | null,
  modifiers: SelectionModifiers,
): string[] {
  if (modifiers.range && anchor) {
    const from = ordered.indexOf(anchor)
    const to = ordered.indexOf(key)
    if (from >= 0 && to >= 0) {
      const [start, end] = from < to ? [from, to] : [to, from]
      return [...new Set([...current, ...ordered.slice(start, end + 1)])]
    }
  }
  if (modifiers.toggle) {
    return current.includes(key)
      ? current.filter((candidate) => candidate !== key)
      : [...current, key]
  }
  return [key]
}

/** Remove only list identities. No filesystem path is accepted by this API. */
export function addRemovedKeys(current: string[], candidates: string[]): string[] {
  return [...new Set([...current, ...candidates])]
}

export function restoreRemovedKeys(current: string[], restoring: string[]): string[] {
  const restored = new Set(restoring)
  return current.filter((key) => !restored.has(key))
}
