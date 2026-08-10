/**
 * MDX → display-only markdown.
 *
 * MDX is markdown + JSX. Feeding it straight to the rich editor renders
 * `import` statements as prose and JSX bodies as stray code blocks. This turns
 * the MDX-specific constructs into fenced code blocks so they read as what they
 * are: code embedded in the document.
 *
 * This output is DISPLAY ONLY — mdx tabs open the rich editor read-only and
 * never serialize back. A wrong guess here costs rendering fidelity, never a
 * byte of the user's file. That is what lets this stay a line scanner instead
 * of a full MDX parser.
 */
const ESM_LINE = /^(?:import|export)\s/
/** Opening or closing fence marker (``` or ~~~, three or more). */
const FENCE = /^ {0,3}(`{3,}|~{3,})/
/**
 * Start of a block-level JSX component: a tag opening at column 0 whose name is
 * capitalised (`<Chart`) or dotted (`<Base.Layout`) — MDX's own rule for
 * telling components from plain HTML. Lowercase tags like `<div>` are left to
 * the markdown renderer, which already handles embedded HTML.
 */
const JSX_OPEN = /^<([A-Z][\w.]*|[a-z][\w]*\.[\w.]+)/

/**
 * Last line index of the JSX block opening at `start`. Self-closing tags end on
 * their own line; otherwise we look for the matching `</Tag>` at column 0. When
 * no close is found the block is treated as one line — display-only output, so
 * an over-eager guess would cost more than an under-eager one.
 */
function jsxBlockEnd(lines: string[], start: number, tag: string): number {
  if (/\/>\s*$/.test(lines[start])) return start
  const close = new RegExp(`^</${tag.replace(/\./g, '\\.')}\\s*>`)
  for (let j = start + 1; j < lines.length; j++) {
    if (close.test(lines[j])) return j
  }
  return start
}

export function toDisplayMarkdown(src: string): string {
  const lines = src.split('\n')
  const out: string[] = []
  let i = 0
  let fence: string | null = null
  while (i < lines.length) {
    const opener = lines[i].match(FENCE)
    if (fence) {
      // Inside a fence: copy verbatim, close only on a matching marker.
      if (opener && opener[1][0] === fence[0] && opener[1].length >= fence.length) fence = null
      out.push(lines[i])
      i++
      continue
    }
    if (opener) {
      fence = opener[1]
      out.push(lines[i])
      i++
      continue
    }
    if (ESM_LINE.test(lines[i])) {
      const start = i
      while (i < lines.length && ESM_LINE.test(lines[i])) i++
      out.push('```jsx', ...lines.slice(start, i), '```')
      continue
    }
    const jsx = lines[i].match(JSX_OPEN)
    if (jsx) {
      const end = jsxBlockEnd(lines, i, jsx[1])
      out.push('```jsx', ...lines.slice(i, end + 1), '```')
      i = end + 1
      continue
    }
    out.push(lines[i])
    i++
  }
  return out.join('\n')
}
