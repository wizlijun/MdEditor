/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect } from 'vitest'
import { splitFrontmatter, frontmatterDetailsHtml, FRONTMATTER_CSS } from './frontmatter-html'

describe('splitFrontmatter', () => {
  it('splits a leading --- block off the body', () => {
    const { frontmatter, body } = splitFrontmatter('---\ntitle: Hi\n---\n\n# Head\n')
    expect(frontmatter).toBe('title: Hi')
    expect(body).toBe('\n# Head\n')
  })

  it('returns null frontmatter for a document without one', () => {
    const md = '# Head\n\nBody.'
    expect(splitFrontmatter(md)).toEqual({ frontmatter: null, body: md })
  })

  it('ignores a --- thematic break further down the document', () => {
    const md = '# Head\n\n---\n\nBody.'
    expect(splitFrontmatter(md).frontmatter).toBeNull()
  })

  it('leaves an unterminated fence as body rather than swallowing the doc', () => {
    const md = '---\ntitle: Hi\n\n# Head'
    expect(splitFrontmatter(md)).toEqual({ frontmatter: null, body: md })
  })

  it('handles CRLF line endings', () => {
    const { frontmatter, body } = splitFrontmatter('---\r\ntitle: Hi\r\n---\r\nBody')
    expect(frontmatter).toBe('title: Hi')
    expect(body).toBe('Body')
  })

  it('treats an empty fence pair as empty frontmatter', () => {
    expect(splitFrontmatter('---\n---\nBody')).toEqual({ frontmatter: '', body: 'Body' })
  })
})

describe('frontmatterDetailsHtml', () => {
  it('wraps the metadata in a COLLAPSED details (no open attribute)', () => {
    const html = frontmatterDetailsHtml('title: Hi\ntype: Note')
    expect(html).toContain('<details class="frontmatter-details">')
    expect(html).not.toMatch(/<details[^>]*\bopen\b/)
  })

  it('lists the top-level keys in the summary', () => {
    const html = frontmatterDetailsHtml('title: Hi\ntype: Note\ntags:\n  - a')
    expect(html).toMatch(/<summary[^>]*>.*title, type, tags.*<\/summary>/)
  })

  it('renders scalars as key/value property rows without a table', () => {
    const html = frontmatterDetailsHtml('title: Hi\ncount: 3')
    expect(html).toContain('<div class="fm-key">title</div><div class="fm-val">Hi</div>')
    expect(html).toContain('<div class="fm-key">count</div><div class="fm-val">3</div>')
    expect(html).not.toContain('<table')
  })

  it('renders scalar list values as chips', () => {
    const html = frontmatterDetailsHtml('tags:\n  - a\n  - b')
    expect(html).toContain('<ul class="fm-list fm-chips"><li>a</li><li>b</li></ul>')
  })

  it('renders wikilinks, Markdown links, and bare URLs with link affordances', () => {
    const html = frontmatterDetailsHtml([
      'related: "[[Roadmap|Plan]]"',
      'reference: "[Docs](https://example.com/guide)"',
      'website: https://example.com',
    ].join('\n'))

    expect(html).toContain('<span class="fm-wikilink">Plan</span>')
    expect(html).toContain('<a class="fm-inline-link" href="https://example.com/guide">Docs</a>')
    expect(html).toContain('<a class="fm-inline-link" href="https://example.com">https://example.com</a>')
  })

  it('renders nested mappings as labelled lines', () => {
    const html = frontmatterDetailsHtml('verified:\n  by: human:bruce\n  at: 2026-08-11')
    expect(html).toContain('<span class="fm-nested-key">by: </span>human:bruce')
    expect(html).toContain('<span class="fm-nested-key">at: </span>2026-08-11')
  })

  it('renders non-key:value regions as markdown', () => {
    const html = frontmatterDetailsHtml('title: Hi\n\nSome **prose** in the block.\n')
    expect(html).toContain('<div class="frontmatter-md">')
    expect(html).toContain('<strong>prose</strong>')
  })

  it('falls back to raw text for a malformed segment', () => {
    const html = frontmatterDetailsHtml('a: [1, 2\nb: x')
    expect(html).toContain('<pre class="frontmatter-raw">')
  })

  it('escapes HTML in keys and values', () => {
    const html = frontmatterDetailsHtml('title: <script>alert(1)</script>')
    expect(html).not.toContain('<script>')
    expect(html).toContain('&lt;script&gt;')
  })

  it('does not turn unsafe Markdown link schemes into anchors', () => {
    const html = frontmatterDetailsHtml('reference: "[bad](javascript:alert(1)) [bad](custom:run) [local](file:///tmp/a.md)"')
    expect(html).not.toContain('href="javascript:')
    expect(html).not.toContain('href="custom:')
    expect(html).not.toContain('href="file:')
  })

  it('emits nothing for an empty block', () => {
    expect(frontmatterDetailsHtml('')).toBe('')
    expect(frontmatterDetailsHtml('\n  \n')).toBe('')
  })
})

describe('FRONTMATTER_CSS', () => {
  it('scopes under .moraya-editor and ships a dark override', () => {
    expect(FRONTMATTER_CSS).toContain('.moraya-editor .frontmatter-details')
    expect(FRONTMATTER_CSS).toContain('.moraya-editor .frontmatter-properties')
    expect(FRONTMATTER_CSS).not.toContain('.frontmatter-table')
    expect(FRONTMATTER_CSS).toContain('@media (prefers-color-scheme: dark)')
  })
})
