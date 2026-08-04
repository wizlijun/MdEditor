/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect } from 'vitest'
import { mountSource } from './source'

describe('kit source mode', () => {
  it('renders textarea + highlight pre and round-trips value', () => {
    const host = document.createElement('div')
    const s = mountSource(host, '# Title\n\nbody', () => {})
    const ta = host.querySelector('textarea')!
    expect(ta.value).toBe('# Title\n\nbody')
    expect(host.querySelector('pre')!.innerHTML).toContain('Title')
    s.setValue('changed')
    expect(s.getValue()).toBe('changed')
    s.destroy()
    expect(host.childElementCount).toBe(0)
  })

  it('fires onChange on input', () => {
    const host = document.createElement('div')
    let last = ''
    const s = mountSource(host, '', (v) => (last = v))
    const ta = host.querySelector('textarea')!
    ta.value = 'abc'
    ta.dispatchEvent(new Event('input'))
    expect(last).toBe('abc')
    s.destroy()
  })

  it('repaints the overlay when the value changes', () => {
    const host = document.createElement('div')
    const s = mountSource(host, '', () => {})
    const ta = host.querySelector('textarea')!
    const pre = host.querySelector('pre')!
    ta.value = '## Heading'
    ta.dispatchEvent(new Event('input'))
    // renderSourceHtml wraps headings in an `h`-classed span.
    expect(pre.innerHTML).toContain('class="h h2"')
    s.destroy()
  })

  it('auto-closes a doubled marker via autoPairInsert', () => {
    const host = document.createElement('div')
    let last = ''
    const s = mountSource(host, '*', (v) => (last = v))
    const ta = host.querySelector('textarea')!
    ta.setSelectionRange(1, 1)
    const ev = new KeyboardEvent('keydown', { key: '*', cancelable: true })
    ta.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(true)
    // `*` typed after an existing `*` completes `** **` and parks the caret between.
    expect(ta.value).toBe('****')
    expect(ta.selectionStart).toBe(2)
    expect(last).toBe('****')
    s.destroy()
  })

  it('leaves ordinary keystrokes to the browser', () => {
    const host = document.createElement('div')
    const s = mountSource(host, 'ab', () => {})
    const ta = host.querySelector('textarea')!
    ta.setSelectionRange(2, 2)
    const ev = new KeyboardEvent('keydown', { key: 'c', cancelable: true })
    ta.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(false)
    expect(ta.value).toBe('ab')
    s.destroy()
  })

  it('applies the placeholder when given', () => {
    const host = document.createElement('div')
    const s = mountSource(host, '', () => {}, 'write here')
    expect(host.querySelector('textarea')!.placeholder).toBe('write here')
    s.destroy()
  })
})
