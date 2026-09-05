// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest'
import {
  findRichRevealTarget,
  getPositionedRichHeadings,
  getRichHeadingElements,
} from './reveal-target'

function host(html: string): HTMLElement {
  const element = document.createElement('div')
  element.innerHTML = `<div class="ProseMirror">${html}</div>`
  return element
}

describe('findRichRevealTarget', () => {
  it('enumerates only direct ProseMirror headings in document order', () => {
    const root = host([
      '<h1>First</h1>',
      '<blockquote><h2>Nested</h2></blockquote>',
      '<p>Body</p>',
      '<h3>Second</h3>',
    ].join(''))

    expect(getRichHeadingElements(root).map((element) => element.textContent))
      .toEqual(['First', 'Second'])
  })

  it('maps only visible TOC heading indices into scroll-content coordinates', () => {
    const root = host('<h1>First</h1><h2>Empty in TOC</h2><h2>Third</h2>')
    const headings = getRichHeadingElements(root)
    vi.spyOn(headings[0], 'getBoundingClientRect').mockReturnValue({ top: 120 } as DOMRect)
    vi.spyOn(headings[1], 'getBoundingClientRect').mockReturnValue({ top: 180 } as DOMRect)
    vi.spyOn(headings[2], 'getBoundingClientRect').mockReturnValue({ top: 260 } as DOMRect)

    expect(getPositionedRichHeadings(root, new Set([0, 2]), 100, 40)).toEqual([
      { headingIndex: 0, position: 60 },
      { headingIndex: 2, position: 200 },
    ])
  })

  it('addresses top-level headings by index even when titles repeat', () => {
    const root = host([
      '<h1>Repeat</h1>',
      '<blockquote><h2>Nested heading is not part of the TOC</h2></blockquote>',
      '<h2><strong>Repeat</strong></h2>',
    ].join(''))

    expect(findRichRevealTarget(root, { text: 'Repeat', headingIndex: 1 }))
      .toBe(root.querySelector('.ProseMirror > h2'))
  })

  it('falls back to rendered text for legacy reveal requests', () => {
    const root = host('<p>Before</p><h2><strong>Destination</strong></h2>')
    expect(findRichRevealTarget(root, { text: 'Destination' }))
      .toBe(root.querySelector('strong'))
  })

  it('uses the text fallback when a stale heading index is out of range', () => {
    const root = host('<h1>Only</h1><p>Fallback anchor</p>')
    expect(findRichRevealTarget(root, { text: 'Fallback anchor', headingIndex: 4 }))
      .toBe(root.querySelector('p'))
  })

  it('returns null when neither address can be resolved', () => {
    expect(findRichRevealTarget(host('<p>Other</p>'), { text: 'Missing' })).toBeNull()
  })
})
