import { Lexer } from 'marked'
import { cleanLineText } from '../search/preview'

export interface TocHeading {
  /** Markdown heading level, 1–6. */
  level: number
  /** Visual tree depth after normalising skipped/leading heading levels. */
  depth: number
  /** 1-based source line of the heading text (the text line for Setext). */
  line: number
  /** Plain text shared by the panel label and rich-editor reveal anchor. */
  text: string
  /** Zero-based index among top-level headings rendered by the rich editor. */
  headingIndex: number
}

function countNewlines(text: string): number {
  let count = 0
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) count++
  }
  return count
}

/**
 * Mask a leading YAML frontmatter block without changing offsets or line
 * numbers. Markdown parsers otherwise interpret a YAML comment such as
 * `# draft` as the document's first H1.
 */
function maskFrontmatter(markdown: string): string {
  const lines = markdown.split(/(?<=\n)/)
  if (lines.length === 0) return markdown
  const first = lines[0].replace(/^\uFEFF/, '').replace(/\r?\n$/, '')
  if (!/^---[ \t]*$/.test(first)) return markdown

  let end = -1
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i].replace(/\r?\n$/, '')
    if (/^(?:---|\.\.\.)[ \t]*$/.test(line)) {
      end = i
      break
    }
  }
  if (end < 0) return markdown

  const prefix = lines.slice(0, end + 1).join('')
  return prefix.replace(/[^\r\n]/g, ' ') + lines.slice(end + 1).join('')
}

/** Extract the visible top-level Markdown headings used by the TOC panel. */
export function extractTocHeadings(markdown: string): TocHeading[] {
  const tokens = Lexer.lex(maskFrontmatter(markdown), { gfm: true })
  const headings: TocHeading[] = []
  const levelStack: number[] = []
  let line = 1
  let headingIndex = 0

  for (const token of tokens) {
    if (token.type === 'heading') {
      const text = cleanLineText(token.text)
      if (text) {
        while (levelStack.length > 0 && levelStack[levelStack.length - 1] >= token.depth) {
          levelStack.pop()
        }
        headings.push({
          level: token.depth,
          depth: levelStack.length,
          line,
          text,
          headingIndex,
        })
        levelStack.push(token.depth)
      }
      headingIndex++
    }
    line += countNewlines(token.raw)
  }

  return headings
}
