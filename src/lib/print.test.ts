import { describe, it, expect } from 'vitest'
import { wrapPrintHtml } from './print'

describe('wrapPrintHtml', () => {
  it('produces a full A4 document embedding pdf.css and the body', () => {
    const out = wrapPrintHtml('<p>hello</p>', 'My Doc')
    expect(out).toContain('<!doctype html>')
    expect(out).toContain('size: A4')           // from plugins-src/md2pdf/assets/pdf.css
    expect(out).toContain('<p>hello</p>')
    expect(out).toContain('<title>My Doc</title>')
    expect(out).toContain('data-pdf-title="My Doc"')
  })

  it('html-escapes the title in both <title> and data-pdf-title', () => {
    const out = wrapPrintHtml('<p>x</p>', 'A & B <c> "d"')
    expect(out).toContain('<title>A &amp; B &lt;c&gt; &quot;d&quot;</title>')
    expect(out).toContain('data-pdf-title="A &amp; B &lt;c&gt; &quot;d&quot;"')
    expect(out).not.toContain('<title>A & B <c>')
  })
})
