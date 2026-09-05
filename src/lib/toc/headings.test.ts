import { describe, expect, it } from 'vitest'
import { extractTocHeadings } from './headings'

describe('extractTocHeadings', () => {
  it('extracts ATX and Setext headings with source lines', () => {
    const markdown = [
      '# Article',
      '',
      'Introduction',
      '------------',
      '',
      '### Details',
    ].join('\n')

    expect(extractTocHeadings(markdown)).toEqual([
      { level: 1, depth: 0, line: 1, text: 'Article', headingIndex: 0 },
      { level: 2, depth: 1, line: 3, text: 'Introduction', headingIndex: 1 },
      { level: 3, depth: 2, line: 6, text: 'Details', headingIndex: 2 },
    ])
  })

  it('normalises skipped and leading levels into a compact tree', () => {
    const headings = extractTocHeadings([
      '### Starts deep',
      '##### Child',
      '#### Sibling',
      '# Reset',
      '### Reset child',
    ].join('\n'))

    expect(headings.map((heading) => [heading.level, heading.depth])).toEqual([
      [3, 0],
      [5, 1],
      [4, 1],
      [1, 0],
      [3, 1],
    ])
  })

  it('ignores leading frontmatter, fenced code and nested block headings', () => {
    const markdown = [
      '---',
      'title: Example',
      'note: |',
      '  ---',
      '  # yaml comment',
      '---',
      '# Real',
      '```md',
      '## fenced',
      '```',
      '~~~',
      '### fenced too',
      '~~~',
      '> ## quoted heading',
      '## End',
    ].join('\n')

    expect(extractTocHeadings(markdown).map(({ text, line }) => ({ text, line }))).toEqual([
      { text: 'Real', line: 7 },
      { text: 'End', line: 15 },
    ])
  })

  it('uses rendered plain text and keeps duplicate headings independently addressable', () => {
    const markdown = [
      '# **Repeat** #',
      '## [[page|Linked label]] and [site](https://example.com)',
      '# Repeat',
    ].join('\n')

    expect(extractTocHeadings(markdown)).toEqual([
      { level: 1, depth: 0, line: 1, text: 'Repeat', headingIndex: 0 },
      { level: 2, depth: 1, line: 2, text: 'Linked label and site', headingIndex: 1 },
      { level: 1, depth: 0, line: 3, text: 'Repeat', headingIndex: 2 },
    ])
  })

  it('preserves line numbers for CRLF documents and skips empty headings', () => {
    const markdown = '# One\r\n\r\n###\r\n## Two\r\n'
    expect(extractTocHeadings(markdown).map(({ text, line, headingIndex }) => ({ text, line, headingIndex }))).toEqual([
      { text: 'One', line: 1, headingIndex: 0 },
      { text: 'Two', line: 4, headingIndex: 2 },
    ])
  })
})
