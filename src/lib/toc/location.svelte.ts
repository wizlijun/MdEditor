export const tocLocation = $state<{
  trackedTabId: string | null
  activeHeadingIndex: number | null
}>({
  trackedTabId: null,
  activeHeadingIndex: null,
})

let owner = 0

/** Start the one tracking session owned by the mounted TOC panel. */
export function beginTocLocationTracking(tabId: string): () => void {
  const currentOwner = ++owner
  tocLocation.trackedTabId = tabId
  tocLocation.activeHeadingIndex = null

  return () => {
    if (owner !== currentOwner) return
    tocLocation.trackedTabId = null
    tocLocation.activeHeadingIndex = null
  }
}

/** Ignore delayed scroll frames from an editor that is no longer tracked. */
export function reportTocLocation(tabId: string, headingIndex: number | null): void {
  if (tocLocation.trackedTabId !== tabId) return
  tocLocation.activeHeadingIndex = headingIndex
}
