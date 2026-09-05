// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { findRichRevealTarget } from './reveal-target'

function host(html: string): HTMLElement {
  const element = document.createElement('div')
  element.innerHTML = `<div class="ProseMirror">${html}</div>`
  return element
}

describe('findRichRevealTarget', () => {
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
