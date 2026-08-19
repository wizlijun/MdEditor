// A one-shot "focus this editor when it mounts" signal, keyed by file path.
//
// Opening a freshly-created file (e.g. a quick note) remounts the editor for a
// new tab id, so a fire-and-forget window event dispatched right after openFile
// races the mount and is usually missed. Keying the request by path lets the
// caller set it BEFORE openFile creates the tab; each editor consumes it on
// mount by its own `tab.filePath`, so timing no longer matters.

const pending = new Set<string>()

/** Ask the editor that will host `path` to grab focus (cursor in edit state)
 *  as soon as it mounts. Call before/around openFile(path). */
export function requestEditorFocus(path: string): void {
  if (path) pending.add(path)
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('notemd:focus-editor', { detail: { path } }))
  }
}

/** Editors call this on mount with their own file path. Returns true exactly
 *  once per request, clearing it so later remounts don't re-focus. */
export function consumeEditorFocus(path: string): boolean {
  return pending.delete(path)
}
