import { isApplePlatformSync } from './platform-sync'

/** Modifier+key fields of a keydown — a structural subset of KeyboardEvent. */
type Chord = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>

/**
 * True for the platform's Select All chord. Resolved the way ProseMirror's
 * `Mod-` resolves it: Cmd on Apple, Ctrl elsewhere. Deliberately NOT
 * `meta || ctrl` — on macOS Ctrl+A moves to the start of the line.
 */
export function isSelectAllShortcut(e: Chord): boolean {
  const mod = isApplePlatformSync() ? e.metaKey : e.ctrlKey
  return mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a'
}

interface SelectableTextControl {
  focus(): void
  select(): void
}

/**
 * Explicitly selects a textarea/input buffer instead of relying on the host
 * WebView's native responder chain. Both main SourceView and Editor Kit use
 * this entry point so their source-mode shortcuts cannot drift apart.
 */
export function handleTextSelectAllKeydown(
  e: Chord & { preventDefault(): void; stopPropagation(): void },
  control: SelectableTextControl,
): boolean {
  if (!isSelectAllShortcut(e)) return false
  e.preventDefault()
  e.stopPropagation()
  control.focus()
  control.select()
  return true
}
