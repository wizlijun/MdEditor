import { CanvasHistory } from './history'

export interface CanvasUiSession {
  readonly history: CanvasHistory
  /** Last canonical JSON string observed by the surface that owns this session. */
  content: string
}

const sessions = new Map<string, CanvasUiSession>()

/**
 * Keep structural undo history with the open tab rather than a mounted view.
 * App.svelte only mounts the active EditorPane, so a component-local history
 * would otherwise disappear on every tab switch.
 */
export function acquireCanvasUiSession(tabId: string, content: string): CanvasUiSession {
  const existing = sessions.get(tabId)
  if (existing) {
    // A content change that did not pass through markCanvasUiSessionContent is
    // an external reload/restore. Old transactions must never be replayed onto
    // that new disk baseline.
    if (existing.content !== content) {
      existing.history.clear()
      existing.content = content
    }
    return existing
  }
  const created = { history: new CanvasHistory(), content }
  sessions.set(tabId, created)
  return created
}

export function markCanvasUiSessionContent(tabId: string, content: string): void {
  const session = sessions.get(tabId)
  if (session) session.content = content
}

export function releaseCanvasUiSession(tabId: string): void {
  sessions.delete(tabId)
}

/** Test-only reset; exported to keep module state deterministic. */
export function clearCanvasUiSessions(): void {
  sessions.clear()
}
