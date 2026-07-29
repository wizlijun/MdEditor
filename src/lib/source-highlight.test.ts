import { describe, it, expect } from 'vitest'
import { renderSourceHtml } from './source-highlight'

describe('renderSourceHtml — without search hits (existing behaviour)', () => {
  it('wraps headings in their level class', () => {
    // Trailing empty line renders as a single space (keeps the <pre> as tall
    // as the textarea) — same as the pre-extraction highlighter.
    expect(renderSourceHtml('# Title\n')).toBe('<span class="h h1"># Title</span>\n \n')
  })

  it('escapes html and keeps plain lines plain', () => {
    expect(renderSourceHtml('a <b> & c')).toBe('a &lt;b&gt; &amp; c\n')
  })

  it('renders an empty line as a single space so the pre keeps its height', () => {
    expect(renderSourceHtml('a\n\nb')).toBe('a\n \nb\n')
  })

  it('tints CriticMarkup highlight + note spans', () => {
    const html = renderSourceHtml('x {==sel==}{>>note<<} y')
    expect(html).toContain('<span class="crit-hl">{==sel==}</span>')
    expect(html).toContain('<span class="crit-note">{&gt;&gt;note&lt;&lt;}</span>')
  })

  it('tints a bare point annotation', () => {
    expect(renderSourceHtml('end{>>n<<}')).toContain('<span class="crit-note">{&gt;&gt;n&lt;&lt;}</span>')
  })
})

describe('renderSourceHtml — search hits', () => {
  it('marks every hit, and the current one distinctly', () => {
    const value = 'alpha beta alpha'
    const hits = [{ start: 0, end: 5 }, { start: 11, end: 16 }]
    const html = renderSourceHtml(value, hits, 1)
    expect(html).toBe('<span class="search-hit">alpha</span> beta <span class="search-hit-current">alpha</span>\n')
  })

  it('escapes hit text', () => {
    const html = renderSourceHtml('a <b> c', [{ start: 2, end: 5 }], 0)
    expect(html).toBe('a <span class="search-hit-current">&lt;b&gt;</span> c\n')
  })

  it('maps hits onto the right line using absolute offsets', () => {
    const value = 'one\ntwo\nthree'
    // "two" starts at offset 4
    const html = renderSourceHtml(value, [{ start: 4, end: 7 }], 0)
    expect(html).toBe('one\n<span class="search-hit-current">two</span>\nthree\n')
  })

  it('clips a hit that spans a line break onto both lines', () => {
    const value = 'ab\ncd'
    const html = renderSourceHtml(value, [{ start: 1, end: 4 }], 0)
    expect(html).toBe(
      'a<span class="search-hit-current">b</span>\n<span class="search-hit-current">c</span>d\n',
    )
  })

  it('combines classes where a hit overlaps a note span instead of nesting tags', () => {
    const value = '{>>abc<<}'
    // hit covers "b" inside the escaped note run
    const html = renderSourceHtml(value, [{ start: 4, end: 5 }], 0)
    expect(html).toBe(
      '<span class="crit-note">{&gt;&gt;a</span>' +
      '<span class="crit-note search-hit-current">b</span>' +
      '<span class="crit-note">c&lt;&lt;}</span>\n',
    )
  })

  it('marks hits inside a heading line', () => {
    const html = renderSourceHtml('# Title', [{ start: 2, end: 7 }], 0)
    expect(html).toBe(
      '<span class="h h1"># </span><span class="h h1 search-hit-current">Title</span>\n',
    )
  })

  it('ignores hits that fall outside every line', () => {
    expect(renderSourceHtml('abc', [{ start: 10, end: 12 }], 0)).toBe('abc\n')
  })

  it('renders unchanged when the hit list is empty', () => {
    expect(renderSourceHtml('abc', [], -1)).toBe('abc\n')
  })
})

describe('renderSourceHtml — mark cap', () => {
  // 1000 single-char hits: "a a a …"
  const value = Array.from({ length: 1000 }, () => 'a').join(' ')
  const hits = Array.from({ length: 1000 }, (_, i) => ({ start: i * 2, end: i * 2 + 1 }))

  it('paints at most maxMarks hits', () => {
    const html = renderSourceHtml(value, hits, 0, 10)
    expect(html.match(/class="search-hit"/g)?.length).toBe(9)
    expect(html.match(/class="search-hit-current"/g)?.length).toBe(1)
  })

  it('keeps the current hit inside the painted window', () => {
    const html = renderSourceHtml(value, hits, 700, 10)
    expect(html).toContain('search-hit-current')
    // window is centred on the current hit, not stuck at the start
    const firstMark = html.indexOf('search-hit')
    expect(firstMark).toBeGreaterThan(value.length / 2)
  })

  it('clamps the window at the end of the list', () => {
    const html = renderSourceHtml(value, hits, 999, 10)
    expect(html.match(/class="search-hit-current"/g)?.length).toBe(1)
    expect(html.match(/class="search-hit"/g)?.length).toBe(9)
  })

  it('does not window when the hit count is under the cap', () => {
    const small = hits.slice(0, 5)
    const html = renderSourceHtml(value, small, 4, 10)
    expect(html.match(/class="search-hit"/g)?.length).toBe(4)
    expect(html.match(/class="search-hit-current"/g)?.length).toBe(1)
  })
})
