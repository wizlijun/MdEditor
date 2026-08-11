/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (_cmd: string, args: { id: string }) => {
    const id = args.id
    if (id === 'default') return '[data-theme="default"] .moraya-editor { color: black; }'
    if (id === 'effie') return '[data-theme="effie"] .moraya-editor { color: teal; } [data-theme="effie"] .moraya-editor h1::before { content: "H1"; display: block; }'
    return ''
  }),
}))

import {
  shareHeaderLabel, isoDateStamp, viewportMetaTag, themeCssBlock,
  guardSize, MAX_HTML_BYTES,
  extractShareDescription, metadataBlock, okfMetaBlock,
} from './share-baker'
import { ShareError } from '../share/types'

describe('shareHeaderLabel', () => {
  it('uses basename for normal paths', () => {
    expect(shareHeaderLabel('/Users/bruce/notes/foo.md')).toBe('foo.md')
  })
  it('keeps dotfile name intact', () => {
    expect(shareHeaderLabel('/proj/.env')).toBe('.env')
  })
  it('uses "Untitled" for null path', () => {
    expect(shareHeaderLabel(null)).toBe('Untitled')
  })
})

describe('isoDateStamp', () => {
  it('produces YYYY-MM-DD from a Date', () => {
    expect(isoDateStamp(new Date('2026-05-08T10:30:00Z'))).toBe('2026-05-08')
  })
})

describe('viewportMetaTag', () => {
  it('returns the standard width=device-width tag', () => {
    expect(viewportMetaTag()).toBe(
      '<meta name="viewport" content="width=device-width, initial-scale=1">'
    )
  })
})

describe('themeCssBlock', () => {
  it('contains light defaults and a prefers-color-scheme dark override', () => {
    const css = themeCssBlock()
    expect(css).toContain('color-scheme:')
    expect(css).toContain('@media (prefers-color-scheme: dark)')
    expect(css).toContain('img { max-width: 100%')
    expect(css).toContain('pre { overflow-x: auto')
  })
})

describe('extractShareDescription', () => {
  it('takes the first prose paragraph after the H1', () => {
    const md = '# Title\n\nThis is the first paragraph.\n\nSecond para.'
    expect(extractShareDescription(md)).toBe('This is the first paragraph.')
  })
  it('skips fenced code blocks', () => {
    const md = '# T\n\n```js\nconst x = 1\n```\n\nReal prose here.'
    expect(extractShareDescription(md)).toBe('Real prose here.')
  })
  it('strips inline markdown', () => {
    const md = 'a **bold** and *italic* and `code` and [link](http://x) word.'
    expect(extractShareDescription(md)).toBe('a bold and italic and code and link word.')
  })
  it('drops images down to alt text', () => {
    const md = 'Before ![alt](x.png) after.'
    expect(extractShareDescription(md)).toBe('Before alt after.')
  })
  it('strips YAML front matter', () => {
    const md = '---\ntitle: Hi\n---\n\nBody text here.'
    expect(extractShareDescription(md)).toBe('Body text here.')
  })
  it('skips list items, blockquotes, table rows, hr', () => {
    const md = '> quoted\n\n- a\n- b\n\n| col | col |\n|---|---|\n| 1 | 2 |\n\n---\n\nFinally prose.'
    expect(extractShareDescription(md)).toBe('Finally prose.')
  })
  it('truncates with ellipsis at word boundary', () => {
    const md = 'Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.'
    const out = extractShareDescription(md, 80)
    expect(out.length).toBeLessThanOrEqual(80)
    expect(out.endsWith('…')).toBe(true)
    expect(out).not.toContain('  ')
  })
  it('returns empty string when no usable prose exists', () => {
    expect(extractShareDescription('# Just a heading\n\n## And another')).toBe('')
    expect(extractShareDescription('```js\ncode only\n```')).toBe('')
  })
})

describe('metadataBlock', () => {
  it('emits title + description + og + twitter tags', () => {
    const out = metadataBlock({ title: 'Hello', description: 'A nice doc.', filename: 'h.md' })
    expect(out).toContain('<title>Hello</title>')
    expect(out).toContain('<meta name="description" content="A nice doc.">')
    expect(out).toContain('<meta property="og:type" content="article">')
    expect(out).toContain('<meta property="og:title" content="Hello">')
    expect(out).toContain('<meta property="og:description" content="A nice doc.">')
    expect(out).toContain('<meta property="og:site_name" content="note.md">')
    expect(out).toContain('<meta name="twitter:card" content="summary">')
    expect(out).toContain('<meta name="twitter:title" content="Hello">')
    expect(out).toContain('<meta name="twitter:description" content="A nice doc.">')
    expect(out).toContain('<meta name="filename" content="h.md">')
  })
  it('falls back to the default note.md logo as og:image / twitter:image', () => {
    const out = metadataBlock({ title: 'T', description: 'D', filename: 'x.md' })
    expect(out).toContain('<meta property="og:image" content="https://raw.githubusercontent.com/wizlijun/note.md/main/src-tauri/icons/64x64.png">')
    expect(out).toContain('<meta property="og:image:width" content="64">')
    expect(out).toContain('<meta property="og:image:height" content="64">')
    expect(out).toContain('<meta property="og:image:alt" content="T">')
    expect(out).toContain('<meta name="twitter:image" content="https://raw.githubusercontent.com/wizlijun/note.md/main/src-tauri/icons/64x64.png">')
  })
  it('honours an explicit imageUrl override', () => {
    const out = metadataBlock({
      title: 'T', description: 'D', filename: 'x.md',
      imageUrl: 'https://example.com/cover.png',
    })
    expect(out).toContain('<meta property="og:image" content="https://example.com/cover.png">')
    expect(out).toContain('<meta name="twitter:image" content="https://example.com/cover.png">')
  })
  it('omits description meta when description is empty', () => {
    const out = metadataBlock({ title: 'T', description: '', filename: 'x.png' })
    expect(out).not.toContain('name="description"')
    expect(out).not.toContain('og:description')
    expect(out).not.toContain('twitter:description')
    expect(out).toContain('<meta property="og:title" content="T">')
    // og:image should still emit even without a description.
    expect(out).toContain('og:image')
  })
  it('escapes HTML-special chars in title and description', () => {
    const out = metadataBlock({
      title: 'A & B <c>',
      description: '"quoted" & <tag>',
      filename: 'x.md',
    })
    expect(out).toContain('A &amp; B &lt;c&gt;')
    expect(out).toContain('&quot;quoted&quot; &amp; &lt;tag&gt;')
    expect(out).not.toContain('<c>')
  })
})

describe('guardSize', () => {
  it('passes through small payloads', () => {
    expect(() => guardSize('x'.repeat(1000))).not.toThrow()
  })
  it("throws ShareError('too_large') for >25MB payloads", () => {
    const big = 'x'.repeat(MAX_HTML_BYTES + 1)
    let err: unknown
    try { guardSize(big) } catch (e) { err = e }
    expect(err).toBeInstanceOf(ShareError)
    expect((err as ShareError).kind).toBe('too_large')
    expect((err as ShareError).detail).toMatch(/^\d+ bytes$/)
  })
})

import { renderTabBody } from './share-baker'
import type { Tab } from '../tabs.svelte'

const fakeTab = (over: Partial<Tab> = {}): Tab => ({
  id: 'x',
  filePath: '/tmp/foo.md',
  title: 'foo.md',
  initialContent: '',
  currentContent: '',
  mode: 'source',
  kind: 'markdown',
  externalState: 'fresh',
  externalBannerDismissed: false,
  lastKnownMtime: 0,
  lastKnownHash: '',
  ...over,
})

describe('renderTabBody', () => {
  it('renders markdown headings to <h1>/<h2>', async () => {
    const t = fakeTab({ currentContent: '# Hello\n\n## World\n\nbody' })
    const body = await renderTabBody(t)
    expect(body).toMatch(/<h1[^>]*>Hello/i)
    expect(body).toMatch(/<h2[^>]*>World/i)
  })

  it('passes HTML tabs through unchanged in body', async () => {
    const t = fakeTab({
      kind: 'html', filePath: '/tmp/foo.html', title: 'foo.html',
      currentContent: '<p>raw</p>',
    })
    const body = await renderTabBody(t)
    expect(body).toContain('<p>raw</p>')
  })

  it('wraps code-kind tabs in a highlighted code block', async () => {
    const t = fakeTab({
      kind: 'code', filePath: '/tmp/foo.py', title: 'foo.py', language: 'python',
      currentContent: 'def f():\n    return 1',
    })
    const body = await renderTabBody(t)
    expect(body).toMatch(/<pre>/)
    expect(body).toContain('language-python')
  })

  it('highlights fenced code blocks via highlight.js', async () => {
    const t = fakeTab({ currentContent: '```js\nconst x = 1\n```' })
    const body = await renderTabBody(t)
    expect(body).toContain('hljs language-js')
  })
})

import { inlineImages, bakeShareHtml, __setImageReaderForTests } from './share-baker'

describe('inlineImages', () => {
  function makeReader(map: Record<string, Uint8Array | Error>) {
    return async (path: string) => {
      const v = map[path]
      if (v instanceof Error) throw v
      if (!v) throw new Error(`fixture missing: ${path}`)
      return v
    }
  }
  function pngFixture(): Uint8Array {
    return new Uint8Array([
      0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00,
    ])
  }

  it('inlines a relative png path as base64 data URL', async () => {
    const html = '<p><img src="./pic.png" alt="ok"></p>'
    const out = await inlineImages(html, '/Users/bruce/notes/foo.md', makeReader({
      '/Users/bruce/notes/pic.png': pngFixture(),
    }))
    expect(out).toContain('src="data:image/png;base64,')
    expect(out).not.toContain('./pic.png')
  })

  it('inlines a file:// absolute path', async () => {
    const html = '<img src="file:///tmp/abs.jpg">'
    const out = await inlineImages(html, '/Users/bruce/notes/foo.md', makeReader({
      '/tmp/abs.jpg': new Uint8Array([0xff, 0xd8, 0xff]),
    }))
    expect(out).toContain('src="data:image/jpeg;base64,')
  })

  it('leaves remote https:// untouched', async () => {
    const html = '<img src="https://example.com/x.png">'
    const out = await inlineImages(html, '/p/foo.md', makeReader({}))
    expect(out).toBe(html)
  })

  it('replaces unreadable image with italic alt text', async () => {
    const html = '<img src="./missing.png" alt="missing alt">'
    const out = await inlineImages(html, '/p/foo.md', makeReader({
      '/p/missing.png': new Error('ENOENT'),
    }))
    expect(out).not.toContain('<img')
    expect(out).toContain('<em>missing alt</em>')
  })

  it('uses [image] placeholder when alt is missing', async () => {
    const html = '<img src="./missing.png">'
    const out = await inlineImages(html, '/p/foo.md', makeReader({
      '/p/missing.png': new Error('ENOENT'),
    }))
    expect(out).toContain('<em>[image]</em>')
  })
})

describe('bakeShareHtml', () => {
  it('produces a full self-contained HTML document', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Hi\n\nbody' })
    const html = await bakeShareHtml(t)
    expect(html.startsWith('<!doctype html>')).toBe(true)
    expect(html).toContain('<meta name="viewport"')
    expect(html).toContain('@media (prefers-color-scheme: dark)')
    expect(html).toContain('<h1')
    expect(html).toContain('class="share-shell"')
    expect(html).toContain('class="share-header"')
    expect(html).toContain('class="share-footer"')
    expect(html).toContain('foo.md')
    __setImageReaderForTests(null)
  })

  it('uses H1 as the page <title> when present, falling back to filename basename', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t1 = fakeTab({ currentContent: '# My Big Topic\n\nbody.' })
    const h1 = await bakeShareHtml(t1)
    expect(h1).toContain('<title>My Big Topic</title>')
    expect(h1).toContain('og:title" content="My Big Topic"')

    const t2 = fakeTab({ currentContent: 'just body, no heading' })
    const h2 = await bakeShareHtml(t2)
    expect(h2).toContain('<title>foo</title>') // basename minus .md
    __setImageReaderForTests(null)
  })

  it('emits og:description and meta description from extracted prose', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Topic\n\nThis is the lead paragraph.\n\nSecond.' })
    const html = await bakeShareHtml(t)
    expect(html).toContain('name="description" content="This is the lead paragraph."')
    expect(html).toContain('og:description" content="This is the lead paragraph."')
    __setImageReaderForTests(null)
  })

  it('keeps the filename in the visible header even when title comes from H1', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Big Idea\n\nbody.' })
    const html = await bakeShareHtml(t)
    // Page title should be "Big Idea", but the visible subtitle keeps "foo.md".
    expect(html).toContain('<title>Big Idea</title>')
    expect(html).toMatch(/class="share-header">foo\.md/)
    __setImageReaderForTests(null)
  })

  it('defaults to the default theme when no theme id is passed', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Hi' })
    const html = await bakeShareHtml(t)
    expect(html).toContain('data-theme="default"')
    expect(html).toContain('[data-theme="default"] .moraya-editor')
    expect(html).toContain('class="moraya-editor"')
    __setImageReaderForTests(null)
  })

  it('inlines effie theme css and sets data-theme="effie" when requested', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Hi' })
    const html = await bakeShareHtml(t, 'effie')
    expect(html).toContain('data-theme="effie"')
    expect(html).toContain('[data-theme="effie"] .moraya-editor')
    __setImageReaderForTests(null)
  })

  it('inlines mobile overrides so phone-width viewports look sane', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Hi' })
    const html = await bakeShareHtml(t, 'effie')
    expect(html).toContain('@media (max-width: 600px)')
    // effie's gutter labels must be hidden on phones — no room for them.
    expect(html).toMatch(/\[data-theme="effie"\][^{]*h1::before[\s\S]*?display: none/)
    __setImageReaderForTests(null)
  })

  it('folds frontmatter into a collapsed details instead of leaking raw YAML', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '---\ntitle: Hi\ntype: Note\n---\n\n# Head\n\nBody.' })
    const html = await bakeShareHtml(t)
    expect(html).toContain('<details class="frontmatter-details">')
    expect(html).not.toMatch(/<details[^>]*\bopen\b/)
    expect(html).toContain('<td class="fm-key">title</td>')
    // The raw block must not survive as prose: no <hr> + YAML paragraph pair.
    expect(html).not.toMatch(/<p>title: Hi/)
    // …and the actual document still renders after it.
    expect(html).toMatch(/<h1[^>]*>Head/)
    expect(html).toContain('.moraya-editor .frontmatter-details')
    __setImageReaderForTests(null)
  })

  it('leaves a frontmatter-less document untouched', async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const t = fakeTab({ currentContent: '# Head\n\n---\n\nAfter the break.' })
    const html = await bakeShareHtml(t)
    expect(html).not.toContain('frontmatter-details"')
    expect(html).toContain('<hr>')
    __setImageReaderForTests(null)
  })

  it("throws ShareError('too_large') for >25MB input", async () => {
    __setImageReaderForTests(async () => new Uint8Array([0]))
    const huge = 'x'.repeat(26 * 1024 * 1024)
    const t = fakeTab({ currentContent: huge })
    const err = await bakeShareHtml(t).then(() => null, (e: unknown) => e)
    expect(err).toBeInstanceOf(ShareError)
    expect((err as ShareError).kind).toBe('too_large')
    expect((err as ShareError).detail).toMatch(/^\d+ bytes$/)
    __setImageReaderForTests(null)
  })
})

function mdTab(): any {
  return {
    id: 't1', filePath: '/notes/foo.md', title: 'foo.md',
    initialContent: '# Title\n\nHello world.', currentContent: '# Title\n\nHello world.',
    mode: 'rich', kind: 'markdown', externalState: 'fresh', externalBannerDismissed: false,
    lastKnownMtime: 0, lastKnownHash: '',
  }
}

describe('bakeShareHtml beacon injection', () => {
  it('always injects the beacon (reading-insights is core)', async () => {
    const html = await bakeShareHtml(mdTab(), 'default')
    expect(html).toContain('/a/hit')
    expect(html).toContain('mdi_vid')
  })
})

describe('okfMetaBlock — 对外产物带出处,但不泄露身份/路径', () => {
  it('publishes the concept type and the derived trust tier', () => {
    const md = [
      '---', 'type: Note', 'title: t',
      'verified: { by: human:bruce, at: 2026-08-04T09:00:00Z }',
      'sources:', '  - resource: /Users/bruce/private/draft.md',
      '---', '', '正文', '',
    ].join('\n')
    const out = okfMetaBlock(md)
    expect(out).toContain('<meta name="okf:type" content="Note">')
    expect(out).toContain('<meta name="okf:trust-tier" content="human-reviewed">')
  })

  it('never leaks actor ids or source paths into a public page', () => {
    const md = '---\ntype: Note\nverified: { by: human:bruce, at: x }\nsources:\n  - resource: /Users/bruce/x.md\n---\n'
    const out = okfMetaBlock(md)
    expect(out).not.toContain('human:bruce')
    expect(out).not.toContain('/Users/bruce')
  })

  it('emits status and stale_after only when they carry information', () => {
    expect(okfMetaBlock('---\ntype: Note\n---\n')).not.toContain('okf:status')
    expect(okfMetaBlock('---\ntype: Note\nstatus: draft\nstale_after: 2026-09-23\n---\n'))
      .toContain('<meta name="okf:status" content="draft">')
    expect(okfMetaBlock('---\ntype: Note\nstale_after: 2026-09-23\n---\n'))
      .toContain('<meta name="okf:stale-after" content="2026-09-23">')
  })

  it('is empty for a document without front-matter', () => {
    expect(okfMetaBlock('# 只有正文\n')).toBe('')
  })

  it('escapes values instead of trusting them', () => {
    expect(okfMetaBlock('---\ntype: \'a"b\'\n---\n')).toContain('content="a&quot;b"')
  })
})
