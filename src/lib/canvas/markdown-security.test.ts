import { describe, expect, it } from 'vitest'
import { CanvasMarkdownResourceGuard, containsRemoteMediaHtml } from './markdown-security'

describe('CanvasMarkdownResourceGuard', () => {
  it('reversibly shields inline, reference, protocol-relative and HTML image sources', () => {
    const markdown = [
      '![inline](https://example.com/a.png)',
      '![cdn](//cdn.example.com/b.png)',
      '![reference][hero]',
      '[hero]: http://example.com/c.png "title"',
      '<img src="https://example.com/d.png" srcset="https://example.com/2x.png 2x">',
      '[ordinary](https://example.com/page)',
    ].join('\n')
    const guard = new CanvasMarkdownResourceGuard()

    const shielded = guard.shield(markdown)

    expect(shielded).not.toMatch(/!\[[^\]]*\]\((?:https?:)?\/\//)
    expect(shielded).not.toMatch(/<img[^>]+(?:https?:)?\/\//)
    expect(shielded).toContain('[ordinary](https://example.com/page)')
    expect(guard.restore(shielded)).toBe(markdown)
  })

  it('recognizes remote media in pasted HTML but not ordinary links', () => {
    expect(containsRemoteMediaHtml('<img src="https://example.com/a.png">')).toBe(true)
    expect(containsRemoteMediaHtml('<video poster="//example.com/a.png"></video>')).toBe(true)
    expect(containsRemoteMediaHtml('<img src="data:image/svg+xml,<svg></svg>">')).toBe(true)
    expect(containsRemoteMediaHtml('<a href="https://example.com">link</a>')).toBe(false)
  })

  it('shields non-network URI schemes and all media source attributes', () => {
    const markdown = [
      '![inline](data:image/svg+xml,<svg/onload=alert(1)>)',
      '<img src="blob:secret" srcset="https://example.com/2x.png 2x">',
      '<video poster="https://example.com/poster.png"><source src="https://example.com/v.mp4"></video>',
    ].join('\n')
    const guard = new CanvasMarkdownResourceGuard()

    const shielded = guard.shield(markdown)

    expect(shielded).not.toContain('data:image/svg+xml')
    expect(shielded).not.toContain('blob:secret')
    expect(shielded).not.toContain('https://example.com')
    expect(guard.restore(shielded)).toBe(markdown)
  })
})
