export interface FindState {
  open: boolean
  showReplace: boolean
  query: string
  replacement: string
  caseSensitive: boolean
  wholeWord: boolean
  useRegex: boolean
  matchCount: number
  currentMatch: number
}

export const findState = $state<FindState>({
  open: false,
  showReplace: false,
  query: '',
  replacement: '',
  caseSensitive: false,
  wholeWord: false,
  useRegex: false,
  matchCount: 0,
  currentMatch: 0,
})

export function openFind() {
  findState.open = true
  findState.showReplace = false
}

export function openFindReplace() {
  findState.open = true
  findState.showReplace = true
}

export function closeFind() {
  findState.open = false
  findState.query = ''
  findState.replacement = ''
  findState.matchCount = 0
  findState.currentMatch = 0
  window.dispatchEvent(new CustomEvent('notemd:find-clear'))
}

export function buildRegex(state: FindState): RegExp | null {
  if (!state.query) return null
  try {
    let pattern = state.useRegex ? state.query : escapeRegex(state.query)
    if (state.wholeWord) pattern = `\\b${pattern}\\b`
    return new RegExp(pattern, state.caseSensitive ? 'g' : 'gi')
  } catch {
    return null
  }
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}
