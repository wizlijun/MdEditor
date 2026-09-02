/**
 * @vitest-environment happy-dom
 */
import { beforeEach, describe, it, expect, vi } from 'vitest'

const platform = vi.hoisted(() => ({ apple: true }))

vi.mock('../lib/platform-sync', () => ({
  isApplePlatformSync: () => platform.apple,
}))

import { mountSource } from './source'

beforeEach(() => {
  platform.apple = true
  document.body.innerHTML = ''
})

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

  it('explicitly selects the whole buffer for Cmd+A on Apple platforms', () => {
    const host = document.createElement('div')
    const bubbled = vi.fn()
    host.addEventListener('keydown', bubbled)
    const s = mountSource(host, 'alpha\nbeta', () => {})
    const ta = host.querySelector('textarea')!
    ta.setSelectionRange(3, 3)

    const ev = new KeyboardEvent('keydown', {
      key: 'a', metaKey: true, bubbles: true, cancelable: true,
    })
    ta.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true)
    expect(bubbled).not.toHaveBeenCalled()
    expect(ta.selectionStart).toBe(0)
    expect(ta.selectionEnd).toBe(ta.value.length)
    s.destroy()
  })

  it('keeps Ctrl+A native on Apple platforms and uses it off Apple platforms', () => {
    const host = document.createElement('div')
    const s = mountSource(host, 'alpha', () => {})
    const ta = host.querySelector('textarea')!
    ta.setSelectionRange(2, 2)

    const appleCtrl = new KeyboardEvent('keydown', {
      key: 'a', ctrlKey: true, bubbles: true, cancelable: true,
    })
    ta.dispatchEvent(appleCtrl)
    expect(appleCtrl.defaultPrevented).toBe(false)
    expect(ta.selectionStart).toBe(2)
    expect(ta.selectionEnd).toBe(2)

    platform.apple = false
    const nonAppleCtrl = new KeyboardEvent('keydown', {
      key: 'A', ctrlKey: true, bubbles: true, cancelable: true,
    })
    ta.dispatchEvent(nonAppleCtrl)
    expect(nonAppleCtrl.defaultPrevented).toBe(true)
    expect(ta.selectionStart).toBe(0)
    expect(ta.selectionEnd).toBe(ta.value.length)
    s.destroy()
  })

  it('applies the placeholder when given', () => {
    const host = document.createElement('div')
    const s = mountSource(host, '', () => {}, 'write here')
    expect(host.querySelector('textarea')!.placeholder).toBe('write here')
    s.destroy()
  })
})
