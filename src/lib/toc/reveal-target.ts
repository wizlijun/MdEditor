interface RichRevealTarget {
  text: string
  headingIndex?: number
}

const HEADING_SELECTOR = 'h1,h2,h3,h4,h5,h6'

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
    const editor = host.querySelector('.ProseMirror')
    const headings = editor
      ? Array.from(editor.children).filter((element) => element.matches(HEADING_SELECTOR))
      : []
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
