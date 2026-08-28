/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import {
  htmlEscape,
  extractH1FromMarkdown,
  buildPdfTitle,
  hasMathContent,
  inlineImages,
  renderTabBody,
  renderTabAsInlineBody,
  renderMarkdownInline,
  __setImageReaderForTests,
} from './host-render-html'

describe('htmlEscape', () => {
  it('escapes the four critical characters', () => {
    expect(htmlEscape('a&b<c>d"e')).toBe('a&amp;b&lt;c&gt;d&quot;e')
  })
  it('passes ASCII through unchanged', () => {
    expect(htmlEscape('plain text 123')).toBe('plain text 123')
  })
})

describe('extractH1FromMarkdown', () => {
  it('returns the first H1 text', () => {
    expect(extractH1FromMarkdown('# Hello\n\nbody')).toBe('Hello')
  })
  it('returns null when no H1 present', () => {
    expect(extractH1FromMarkdown('\n\nNo heading here')).toBeNull()
  })
  it('strips trailing closing #s', () => {
    expect(extractH1FromMarkdown('# Title ##')).toBe('Title')
  })
  it('does NOT recognise setext (===) underlines', () => {
    expect(extractH1FromMarkdown('Title\n===')).toBeNull()
  })
})

describe('buildPdfTitle', () => {
  it('uses H1 when present in markdown tab', () => {
    expect(buildPdfTitle({
      kind: 'markdown', currentContent: '# H1\nbody', filePath: '/tmp/foo.md',
    } as never)).toBe('H1')
  })
  it('falls back to basename without extension', () => {
    expect(buildPdfTitle({
      kind: 'markdown', currentContent: 'no heading', filePath: '/tmp/foo.md',
    } as never)).toBe('foo')
  })
  it('keeps the dotfile basename intact', () => {
    expect(buildPdfTitle({
      kind: 'markdown', currentContent: '', filePath: '/proj/.env',
    } as never)).toBe('.env')
  })
  it('uses basename for html tab even with H1 in body', () => {
    expect(buildPdfTitle({
      kind: 'html', currentContent: '<h1>X</h1>', filePath: '/tmp/page.html',
    } as never)).toBe('page')
  })
})

describe('hasMathContent', () => {
  it('detects $ inline math', () => {
    expect(hasMathContent('cost is $E=mc^2$ in physics')).toBe(true)
  })
  it('detects $$ display math', () => {
    expect(hasMathContent('text\n$$\\int_0^1 x dx$$\n')).toBe(true)
  })
  it('detects \\(...\\)', () => {
    expect(hasMathContent('inline \\(a+b\\)')).toBe(true)
  })
  it('returns false for plain prose', () => {
    expect(hasMathContent('no math here, just words.')).toBe(false)
  })
  it('returns false for prose with isolated dollar signs', () => {
    expect(hasMathContent('cost is $5 today')).toBe(false)
  })
})

describe('inlineImages', () => {
  beforeEach(() => __setImageReaderForTests(async () => new Uint8Array([0x89, 0x50, 0x4e, 0x47])))
  afterEach(() => __setImageReaderForTests(null))

  it('replaces relative-path <img> with data: URL', async () => {
    const html = '<p><img src="./foo.png" alt="x"></p>'
    const out = await inlineImages(
      html, '/Users/bruce/notes/doc.md',
      async () => new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
    )
    expect(out).toMatch(/data:image\/png;base64,[A-Za-z0-9+/=]+/)
  })
  it('leaves https:// images untouched', async () => {
    const html = '<p><img src="https://x.test/a.png"></p>'
    const out = await inlineImages(html, '/foo/bar.md', async () => new Uint8Array())
    expect(out).toContain('https://x.test/a.png')
    expect(out).not.toContain('data:')
  })
  it('replaces unreadable image with <em>alt</em>', async () => {
    const html = '<p><img src="./missing.png" alt="oops"></p>'
    const out = await inlineImages(html, '/x.md', async () => { throw new Error('enoent') })
    expect(out).toContain('<em>oops</em>')
  })
  it('returns input unchanged when tabPath is null', async () => {
    const html = '<p><img src="./x.png"></p>'
    const out = await inlineImages(html, null, async () => new Uint8Array())
    expect(out).toBe(html)
  })
})

describe('line breaks', () => {
  it('renders multi-line blockquotes with <br> instead of merging to one line', async () => {
    const tab = { kind: 'markdown', currentContent: '> Line one\n> Line two\n> Line three\n', filePath: '/tmp/q.md' } as never
    const html = await renderTabBody(tab)
    expect(html).toContain('<br>')
    expect(html).toContain('Line one')
    expect(html).toContain('Line two')
  })

  it('renders in-paragraph soft breaks as <br> (matches the editor model)', async () => {
    const tab = { kind: 'markdown', currentContent: 'first line\nsecond line\n', filePath: '/tmp/p.md' } as never
    const html = await renderTabBody(tab)
    expect(html).toContain('<br>')
  })
})

describe('Mermaid export rendering', () => {
  it('renders a Unicode quadrant into inline SVG for share and PDF consumers', async () => {
    const tab = {
      kind: 'markdown',
      currentContent: `\`\`\`mermaid
quadrantChart
  x-axis 低触达 --> 高触达
  y-axis 低参与 --> 高参与
  quadrant-1 扩大投入
  活动甲: [0.3, 0.6]
\`\`\``,
      filePath: '/tmp/quadrant.md',
    } as never

    const html = await renderTabAsInlineBody(tab)
    expect(html).toContain('aria-roledescription="quadrantChart"')
    expect(html).toContain('活动甲')
    expect(html).not.toContain('renderer-error')
  })
})

describe('mdx rendering', () => {
  it('renders JSX as code, not as live markup', async () => {
    // Export/share paths must agree with the reading view: MDX components are
    // shown as the code they are, never emitted into the output document.
    const tab = {
      kind: 'mdx',
      currentContent: "import Chart from './c'\n\n# T\n\n<Chart data={x} />\n",
      filePath: '/tmp/a.mdx',
    } as never
    const html = await renderTabBody(tab)
    expect(html).toContain('language-jsx')   // import + JSX landed in code blocks
    expect(html).toContain('&lt;')            // the component tag is escaped text
    expect(html).not.toContain('<Chart')      // ...never live markup
    expect(html).toContain('<h1>T</h1>')      // real markdown still renders
  })
})

describe('highlight rendering', () => {
  it('renders ^^text^^ as <mark>text</mark>', async () => {
    const tab = { kind: 'markdown', currentContent: 'Hello ^^world^^ end\n', filePath: '/tmp/test.md' } as never
    const html = await renderTabBody(tab)
    expect(html).toContain('<mark>world</mark>')
  })

  it('renders ==text== as <mark>text</mark>', async () => {
    const tab = { kind: 'markdown', currentContent: 'Hello ==world== end\n', filePath: '/tmp/test.md' } as never
    const html = await renderTabBody(tab)
    expect(html).toContain('<mark>world</mark>')
  })

  it('does not XSS on malicious ^^content^^', async () => {
    const tab = { kind: 'markdown', currentContent: '^^<script>alert(1)</script>^^\n', filePath: '/tmp/test.md' } as never
    const html = await renderTabBody(tab)
    expect(html).not.toContain('<script>')
    expect(html).toContain('&lt;script&gt;')
  })
})

describe('CriticMarkup annotations in exported HTML', () => {
  it('renders wrapped annotation as mark + badge with title', () => {
    const html = renderMarkdownInline('a {==bc==}{>>my note<<} d')
    expect(html).toContain('<mark class="crit-anno">bc</mark>')
    expect(html).toContain('class="crit-badge" title="my note"')
  })

  it('renders point annotation as badge only', () => {
    const html = renderMarkdownInline('end{>>hi<<}')
    expect(html).not.toContain('crit-anno')
    expect(html).toContain('class="crit-badge" title="hi"')
  })

  it('escapes hostile note text in the title attribute', () => {
    const html = renderMarkdownInline('x{>>a "b" <i> & c<<}')
    expect(html).toContain('title="a &quot;b&quot; &lt;i&gt; &amp; c"')
  })

  it('keeps inline formatting inside the annotated text', () => {
    const html = renderMarkdownInline('{==has **bold**==}{>>n<<}')
    expect(html).toContain('<strong>bold</strong>')
  })

  it('leaves incomplete markers untouched (fail open)', () => {
    const html = renderMarkdownInline('x {>>never closed')
    expect(html).not.toContain('crit-badge')
  })

  it('renders a question annotation (note contains ?) with the ⁇ badge', () => {
    const half = renderMarkdownInline('a {==bc==}{>>why though?<<} d')
    expect(half).toContain('>⁇</sup>')
    expect(half).not.toContain('>※</sup>')
    const full = renderMarkdownInline('end{>>这样对吗？<<}')
    expect(full).toContain('>⁇</sup>')
  })

  it('keeps the ※ badge for a plain (non-question) note', () => {
    const html = renderMarkdownInline('a {==bc==}{>>just a note<<} d')
    expect(html).toContain('>※</sup>')
    expect(html).not.toContain('>⁇</sup>')
  })
})
