import { afterEach, describe, expect, it } from 'vitest'
import { frontmatterInlineParts } from './frontmatter-inline'
import { setBlockedWikilinks } from './wikilink/blocklist'

afterEach(() => setBlockedWikilinks([]))

describe('frontmatterInlineParts', () => {
  it('recognises each supported link form once and preserves its raw source', () => {
    expect(frontmatterInlineParts('[[Roadmap|Plan]] [Docs](guide.md) https://example.com')).toEqual([
      { kind: 'wikilink', raw: '[[Roadmap|Plan]]', target: 'Roadmap', label: 'Plan' },
      { kind: 'text', text: ' ' },
      { kind: 'link', raw: '[Docs](guide.md)', href: 'guide.md', label: 'Docs' },
      { kind: 'text', text: ' ' },
      { kind: 'url', raw: 'https://example.com', href: 'https://example.com' },
    ])
  })

  it('does not autolink the URL inside a Markdown link', () => {
    expect(frontmatterInlineParts('[Docs](https://example.com/guide)')).toEqual([
      {
        kind: 'link',
        raw: '[Docs](https://example.com/guide)',
        href: 'https://example.com/guide',
        label: 'Docs',
      },
    ])
  })

  it('keeps balanced parentheses inside a Markdown destination', () => {
    expect(frontmatterInlineParts('[Docs](https://example.com/Foo_(bar))')).toEqual([
      {
        kind: 'link',
        raw: '[Docs](https://example.com/Foo_(bar))',
        href: 'https://example.com/Foo_(bar)',
        label: 'Docs',
      },
    ])
  })

  it('leaves sentence punctuation outside a bare URL', () => {
    expect(frontmatterInlineParts('See https://example.com/docs.')).toEqual([
      { kind: 'text', text: 'See ' },
      { kind: 'url', raw: 'https://example.com/docs', href: 'https://example.com/docs' },
      { kind: 'text', text: '.' },
    ])
  })

  it('keeps blocked wikilinks and unsafe schemes as plain text', () => {
    setBlockedWikilinks(['Roadmap'])
    const input = '[[Roadmap|Plan]] [bad](javascript:alert(1)) [bad](data:text/html,x) [bad](custom:run)'
    expect(frontmatterInlineParts(input)).toEqual([{ kind: 'text', text: input }])
  })

  it('allows the explicit desktop protocols and local paths', () => {
    expect(frontmatterInlineParts('[mail](mailto:a@example.com) [note](notes/a.md) [file](file:///tmp/a.md)'))
      .toEqual([
        { kind: 'link', raw: '[mail](mailto:a@example.com)', href: 'mailto:a@example.com', label: 'mail' },
        { kind: 'text', text: ' ' },
        { kind: 'link', raw: '[note](notes/a.md)', href: 'notes/a.md', label: 'note' },
        { kind: 'text', text: ' ' },
        { kind: 'link', raw: '[file](file:///tmp/a.md)', href: 'file:///tmp/a.md', label: 'file' },
      ])
  })

  it('never interprets HTML as markup', () => {
    expect(frontmatterInlineParts('<img src=x onerror=alert(1)>')).toEqual([
      { kind: 'text', text: '<img src=x onerror=alert(1)>' },
    ])
  })
})
