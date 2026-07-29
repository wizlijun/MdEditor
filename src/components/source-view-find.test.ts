/**
 * @vitest-environment happy-dom
 */
// Mount-time find behaviour: switching rich/source (or tabs) mounts a new
// editor while the find bar keeps its query, and the bar only dispatches when
// the query changes — so the editor has to pull the current state on mount.
import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null), convertFileSrc: (s: string) => s }))
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))
vi.mock('@tauri-apps/plugin-fs', () => ({ exists: async () => false, readTextFile: async () => '', writeTextFile: async () => {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => null, save: async () => null }))

const CONTENT = Array.from({ length: 40 }, (_, i) => `line ${i} alpha beta`).join('\n')

async function mountSourceView() {
  const { mount } = await import('svelte')
  const SourceView = (await import('./SourceView.svelte')).default
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(SourceView, { target, props: { value: CONTENT, oninput: () => {}, tabId: 'tab-1' } })
  await new Promise((r) => setTimeout(r, 20))
  return target
}

describe('SourceView find integration', () => {
  beforeEach(async () => {
    const { findState } = await import('../lib/find-replace.svelte')
    findState.open = false
    findState.query = ''
    findState.caseSensitive = false
    findState.wholeWord = false
    findState.useRegex = false
    findState.matchCount = 0
    findState.currentMatch = 0
  })

  it('picks up a query that was already in the find bar when it mounts', async () => {
    const { findState, openFind } = await import('../lib/find-replace.svelte')
    openFind()
    findState.query = 'alpha'

    await mountSourceView()

    expect(findState.matchCount).toBe(40)
    expect(findState.currentMatch).toBe(1)
  })

  it('stays quiet when the find bar is closed', async () => {
    const { findState } = await import('../lib/find-replace.svelte')
    findState.query = 'alpha'   // stale query, bar not open
    await mountSourceView()
    expect(findState.matchCount).toBe(0)
  })

  it('paints hits into the overlay, and next moves the current hit', async () => {
    const { findState, openFind } = await import('../lib/find-replace.svelte')
    openFind()
    findState.query = 'alpha'
    const target = await mountSourceView()

    const pre = target.querySelector('pre.hl') as HTMLElement
    // Which hit (in document order) currently carries the "current" class?
    const currentPos = () => {
      const all = [...pre.querySelectorAll('.search-hit, .search-hit-current')]
      return all.findIndex((el) => el.classList.contains('search-hit-current'))
    }
    expect(pre.querySelectorAll('.search-hit-current').length).toBe(1)
    expect(pre.querySelectorAll('.search-hit').length).toBe(39)
    expect(currentPos()).toBe(0)

    window.dispatchEvent(new CustomEvent('mdeditor:find-next'))
    await new Promise((r) => setTimeout(r, 10))

    expect(findState.currentMatch).toBe(2)
    expect(currentPos()).toBe(1)
  })

  it('does not loop: one search dispatch produces one search', async () => {
    const { findState, openFind } = await import('../lib/find-replace.svelte')
    openFind()
    await mountSourceView()

    let dispatched = 0
    const count = () => { dispatched++ }
    window.addEventListener('mdeditor:find-search', count)
    window.dispatchEvent(new CustomEvent('mdeditor:find-search', {
      detail: { query: 'alpha', caseSensitive: false, wholeWord: false, useRegex: false },
    }))
    await new Promise((r) => setTimeout(r, 20))
    window.removeEventListener('mdeditor:find-search', count)

    expect(dispatched).toBe(1)
    expect(findState.matchCount).toBe(40)
  })
})
