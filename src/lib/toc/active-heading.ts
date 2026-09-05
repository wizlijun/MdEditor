export interface PositionedHeading {
  headingIndex: number
  position: number
}

/**
 * Pick the latest heading that has crossed the viewport's reading marker.
 * Positions must be supplied in document order. At the absolute bottom the
 * final heading wins even when a short last section cannot reach the marker.
 */
export function resolveActiveHeadingIndex(
  headings: readonly PositionedHeading[],
  markerPosition: number,
  atEnd = false,
): number | null {
  if (headings.length === 0) return null
  if (atEnd) return headings[headings.length - 1].headingIndex

  let active: number | null = null
  for (const heading of headings) {
    if (!Number.isFinite(heading.position)) continue
    if (heading.position > markerPosition) break
    active = heading.headingIndex
  }
  return active
}

/** Convert a source textarea's vertical reading marker into a 1-based line. */
export function sourceViewportAnchorLine(
  scrollTop: number,
  clientHeight: number,
  lineHeight: number,
  paddingTop = 0,
): number {
  if (!Number.isFinite(lineHeight) || lineHeight <= 0) return 0
  const contentY = scrollTop + clientHeight / 2 - paddingTop
  return Math.max(0, Math.floor(contentY / lineHeight) + 1)
}

/** Browsers occasionally report a zero scrollHeight before layout settles. */
export function isScrollAtEnd(
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
): boolean {
  return scrollHeight > clientHeight + 1
    && scrollTop + clientHeight >= scrollHeight - 1
}
