import type { PositionedHeading } from './active-heading'

interface RichRevealTarget {
  text: string
  headingIndex?: number
}

const HEADING_SELECTOR = 'h1,h2,h3,h4,h5,h6'

/** Top-level headings share one index space with the Markdown TOC parser. */
export function getRichHeadingElements(host: HTMLElement): Element[] {
  const editor = host.querySelector('.ProseMirror')
  return editor
    ? Array.from(editor.children).filter((element) => element.matches(HEADING_SELECTOR))
    : []
}

export function getPositionedRichHeadings(
  host: HTMLElement,
  allowedHeadingIndexes: ReadonlySet<number>,
  scrollerTop: number,
  scrollTop: number,
): PositionedHeading[] {
  return getRichHeadingElements(host).flatMap((element, headingIndex) => {
    if (!allowedHeadingIndexes.has(headingIndex)) return []
    return [{
      headingIndex,
      position: element.getBoundingClientRect().top - scrollerTop + scrollTop,
    }]
  })
}

/**
 * Resolve a reveal request against the rendered rich-editor DOM. TOC requests
 * address top-level headings by index so duplicate titles remain distinct;
 * existing search/hand-note requests keep the text-node fallback.
 */
export function findRichRevealTarget(
  host: HTMLElement,
  request: RichRevealTarget,
): Element | null {
  if (request.headingIndex != null) {
    const headings = getRichHeadingElements(host)
    const addressed = headings[request.headingIndex]
    if (addressed) return addressed
  }

  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT)
  while (walker.nextNode()) {
    const text = walker.currentNode as Text
    if (text.textContent?.includes(request.text)) return text.parentElement
  }
  return null
}
