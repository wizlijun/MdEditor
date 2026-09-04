/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi } from 'vitest'
import { renderFrontmatter, buildFrontmatterDom, buildFrontmatterView } from './frontmatter-view'

describe('renderFrontmatter — rendering', () => {
  it('renders key: value pairs as borderless properties with editable scalar values', () => {
    const el = buildFrontmatterDom('title: Hello\nauthor: Bruce')
    const rows = el.querySelectorAll('.frontmatter-properties .fm-property')
    expect(rows.length).toBe(2)
    expect(rows[0].querySelector('.fm-key')?.textContent).toBe('title')
    const val = rows[0].querySelector('.fm-val') as HTMLElement
    expect(val.textContent).toBe('Hello')
    expect(val.getAttribute('contenteditable')).toBe('true')
    expect(el.querySelector('table, tr, td')).toBeNull()
  })

  it('renders a scalar list read-only as rounded chips', () => {
    const el = buildFrontmatterDom('tags:\n  - a\n  - b\n  - c')
    const items = el.querySelectorAll('.fm-val ul.fm-list.fm-chips > li')
    expect(items.length).toBe(3)
    expect(Array.from(items).map(li => li.textContent)).toEqual(['a', 'b', 'c'])
    // Complex values are not inline-editable.
    expect(el.querySelector('.fm-val')?.getAttribute('contenteditable')).toBeNull()
  })

  it('keeps a multi-line (block scalar) string read-only with newlines', () => {
    const el = buildFrontmatterDom('desc: |\n  line one\n  line two\n')
    const val = el.querySelector('.fm-val')!
    expect(val.textContent).toContain('line one\nline two')
    expect(val.getAttribute('contenteditable')).toBeNull()
  })

  it('renders non-key:value regions as markdown', () => {
    const raw = 'title: Hello\n\n> a quote line\n'
    const el = renderFrontmatter(raw)
    // property row for title
    expect(el.querySelector('.frontmatter-properties .fm-key')?.textContent).toBe('title')
    // md block rendered the blockquote
    const md = el.querySelector('.frontmatter-md')!
    expect(md.innerHTML).toContain('<blockquote>')
    expect(md.textContent).toContain('a quote line')
  })

  it('segments mixed content into multiple property groups', () => {
    const raw = 'title: A\n\nprose\n\ndate: B\ntags:\n  - x\n'
    const el = renderFrontmatter(raw)
    expect(el.querySelectorAll('.frontmatter-properties').length).toBe(2)
    expect(el.querySelectorAll('.frontmatter-md').length).toBe(1)
  })

  it('decorates wikilinks, Markdown links, and bare URLs without changing editable text', () => {
    const raw = [
      'related: "[[Roadmap|Plan]]"',
      'reference: "[Docs](notes/guide.md)"',
      'website: https://example.com/docs',
    ].join('\n')
    const el = renderFrontmatter(raw)

    const wiki = el.querySelector('[data-wikilink="Roadmap"]') as HTMLElement
    expect(wiki.classList.contains('wikilink')).toBe(true)
    expect(wiki.textContent).toBe('[[Roadmap|Plan]]')
    expect(wiki.dataset.fmLabel).toBe('Plan')

    const mdLink = el.querySelector('a[href="notes/guide.md"]') as HTMLAnchorElement
    expect(mdLink.textContent).toBe('[Docs](notes/guide.md)')
    expect(mdLink.dataset.fmLabel).toBe('Docs')

    const url = el.querySelector('[data-url="https://example.com/docs"]') as HTMLElement
    expect(url.classList.contains('url-autolink')).toBe(true)
    expect(url.textContent).toBe('https://example.com/docs')

    expect(Array.from(el.querySelectorAll('.fm-val')).map((v) => v.textContent)).toEqual([
      '[[Roadmap|Plan]]',
      '[Docs](notes/guide.md)',
      'https://example.com/docs',
    ])
  })

  it('decorates links inside scalar list chips and nested values', () => {
    const el = renderFrontmatter([
      'related:',
      '  - "[[Roadmap]]"',
      '  - https://example.com',
      'source:',
      '  docs: "[Guide](guide.md)"',
    ].join('\n'))

    expect(el.querySelector('.fm-chips [data-wikilink="Roadmap"]')).toBeTruthy()
    expect(el.querySelector('.fm-chips [data-url="https://example.com"]')).toBeTruthy()
    expect(el.querySelector('.fm-nested a[href="guide.md"]')).toBeTruthy()
  })
})

describe('renderFrontmatter — editing', () => {
  it('writes an edited scalar back into the full raw YAML on blur', () => {
    const onChange = vi.fn()
    const raw = 'title: Hello\ncount: 3\n'
    const el = renderFrontmatter(raw, onChange)
    const countCell = Array.from(el.querySelectorAll('.fm-property'))
      .find(row => row.querySelector('.fm-key')?.textContent === 'count')!
      .querySelector('.fm-val') as HTMLElement

    countCell.textContent = '5'
    countCell.dispatchEvent(new Event('blur'))

    expect(onChange).toHaveBeenCalledTimes(1)
    const newRaw = onChange.mock.calls[0][0] as string
    expect(newRaw).toContain('count: 5')
    expect(newRaw).toContain('title: Hello')
  })

  it('does not fire onChange when the value is unchanged', () => {
    const onChange = vi.fn()
    const el = renderFrontmatter('title: Hello\n', onChange)
    const cell = el.querySelector('.fm-val') as HTMLElement
    cell.dispatchEvent(new Event('blur'))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('does not rewrite an unchanged link scalar on blur', () => {
    const onChange = vi.fn()
    const el = renderFrontmatter('reference: "[Docs](guide.md)"\n', onChange)
    const cell = el.querySelector('.fm-val') as HTMLElement
    expect(cell.textContent).toBe('[Docs](guide.md)')
    cell.dispatchEvent(new Event('blur'))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('restores link decoration after Escape without writing YAML', () => {
    const onChange = vi.fn()
    const el = renderFrontmatter('related: "[[Roadmap|Plan]]"\n', onChange)
    const cell = el.querySelector('.fm-val') as HTMLElement
    cell.focus()
    cell.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))

    expect(cell.querySelector('[data-wikilink="Roadmap"]')).toBeTruthy()
    expect(cell.textContent).toBe('[[Roadmap|Plan]]')
    expect(onChange).not.toHaveBeenCalled()
  })

  it('preserves other keys and comments when editing one value', () => {
    const onChange = vi.fn()
    const raw = 'title: Old # keep me\ncount: 3\n'
    const el = renderFrontmatter(raw, onChange)
    const titleCell = el.querySelector('.fm-val') as HTMLElement
    titleCell.textContent = 'New'
    titleCell.dispatchEvent(new Event('blur'))
    const newRaw = onChange.mock.calls[0][0] as string
    expect(newRaw).toContain('title: New')
    expect(newRaw).toContain('# keep me')
    expect(newRaw).toContain('count: 3')
  })
})

describe('buildFrontmatterView — collapse', () => {
  it('defaults to collapsed (details not open) with a keys summary', () => {
    const container = document.createElement('div')
    const details = buildFrontmatterView(container, 'title: Hello\nauthor: Bruce\n')
    expect(details.tagName).toBe('DETAILS')
    expect((details as HTMLDetailsElement).open).toBe(false)
    expect(details.querySelector('.frontmatter-summary-title')?.textContent).toBe('Metadata')
    const summary = details.querySelector('.frontmatter-summary-keys')!
    expect(summary.textContent).toBe('title, author')
    // Body (the property list) is present inside, ready to reveal on expand.
    expect(details.querySelector('.frontmatter-properties')).toBeTruthy()
  })

  it('honours a previously-open state stashed on the container', () => {
    const container = document.createElement('div')
    container.dataset.fmOpen = '1'
    const details = buildFrontmatterView(container, 'title: Hello\n') as HTMLDetailsElement
    expect(details.open).toBe(true)
  })

  it('persists the open state back to the container on toggle', () => {
    const container = document.createElement('div')
    const details = buildFrontmatterView(container, 'title: Hello\n') as HTMLDetailsElement
    details.open = true
    details.dispatchEvent(new Event('toggle'))
    expect(container.dataset.fmOpen).toBe('1')
  })
})

describe('renderFrontmatter — fallback', () => {
  it('falls back to raw <pre> for a malformed kv segment', () => {
    const el = renderFrontmatter('key: "unterminated\n')
    // Malformed YAML in a kv-looking segment → raw fallback somewhere in output.
    expect(el.querySelector('.frontmatter-raw')).toBeTruthy()
  })
})
