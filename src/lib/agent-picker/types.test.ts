import { describe, it, expect } from 'vitest'
import { placeMenu, providerKey, rememberProvider, rememberedProvider } from './types'

/** An in-memory Storage stand-in; no jsdom localStorage required. */
function store(seed: Record<string, string> = {}) {
  const m = new Map(Object.entries(seed))
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    seen: m,
  }
}

const BOTH = ['notemd.claude-agent', 'notemd.deepseek-agent']

describe('remembering one surface’s agent', () => {
  it('falls back to the host default when nothing was chosen yet', () => {
    expect(rememberedProvider('note', BOTH, 'notemd.claude-agent', store())).toBe(
      'notemd.claude-agent',
    )
  })

  it('returns the choice that was saved for that surface', () => {
    const s = store({ 'notemd.agent.provider.note': 'notemd.deepseek-agent' })
    expect(rememberedProvider('note', BOTH, 'notemd.claude-agent', s)).toBe(
      'notemd.deepseek-agent',
    )
  })

  /// The whole point of per-surface memory: two surfaces, two answers.
  it('keeps surfaces independent', () => {
    const s = store()
    rememberProvider('note', 'notemd.claude-agent', s)
    rememberProvider('ebook', 'notemd.deepseek-agent', s)
    expect(rememberedProvider('note', BOTH, 'notemd.claude-agent', s)).toBe(
      'notemd.claude-agent',
    )
    expect(rememberedProvider('ebook', BOTH, 'notemd.claude-agent', s)).toBe(
      'notemd.deepseek-agent',
    )
  })

  /// Uninstalling a plugin must not wedge the button beside it.
  it('falls back when the remembered agent is no longer installed', () => {
    const s = store({ 'notemd.agent.provider.note': 'notemd.deepseek-agent' })
    expect(
      rememberedProvider('note', ['notemd.claude-agent'], 'notemd.claude-agent', s),
    ).toBe('notemd.claude-agent')
  })

  it('falls back to whatever IS installed when the default is gone too', () => {
    const s = store()
    expect(
      rememberedProvider('note', ['notemd.deepseek-agent'], 'notemd.claude-agent', s),
    ).toBe('notemd.deepseek-agent')
  })

  it('still answers when nothing is installed at all', () => {
    expect(rememberedProvider('note', [], 'notemd.claude-agent', store())).toBe(
      'notemd.claude-agent',
    )
  })

  /// A webview with storage disabled must still render a working picker.
  it('survives storage that throws', () => {
    const hostile = {
      getItem: () => {
        throw new Error('denied')
      },
      setItem: () => {
        throw new Error('denied')
      },
    }
    expect(rememberedProvider('note', BOTH, 'notemd.claude-agent', hostile)).toBe(
      'notemd.claude-agent',
    )
    expect(() => rememberProvider('note', 'notemd.deepseek-agent', hostile)).not.toThrow()
  })

  it('namespaces its keys', () => {
    expect(providerKey('note')).toBe('notemd.agent.provider.note')
    expect(providerKey('idea')).not.toBe(providerKey('note'))
  })
})

describe('placing the menu', () => {
  const VIEW = { width: 1000, height: 800 }
  const MENU = { width: 220, height: 120 }
  /** A button 20px tall, right-of-centre — the shape every surface uses. */
  const button = (top: number, left = 700) => ({ top, left, width: 90, height: 20 })

  it('opens below and right-aligned to the button by default', () => {
    const p = placeMenu(button(100), MENU, VIEW)
    expect(p.side).toBe('down')
    expect(p.top).toBe(124) // 100 + 20 + 4
    expect(p.left).toBe(570) // 700 + 90 - 220
  })

  /// A row near the bottom of a long ebook queue: the whole reason for flipping.
  it('flips up when there is no room below', () => {
    const p = placeMenu(button(750), MENU, VIEW)
    expect(p.side).toBe('up')
    expect(p.top).toBe(626) // 750 - 4 - 120
  })

  it('stays down when below fits, even with more room above', () => {
    const p = placeMenu(button(600), MENU, VIEW)
    expect(p.side).toBe('down')
    expect(p.top).toBe(624)
  })

  /// The narrow sidecar panel: right-aligning would put the menu off-screen.
  it('switches to left-aligned rather than running off the left edge', () => {
    const p = placeMenu(button(100, 20), MENU, VIEW)
    expect(p.left).toBe(20) // anchor.left, not 20 + 90 - 220 = -110
  })

  it('never runs off the right edge either', () => {
    const p = placeMenu(button(100, 960), MENU, { width: 1000, height: 800 })
    expect(p.left).toBeLessThanOrEqual(1000 - MENU.width - 8)
    expect(p.left).toBeGreaterThanOrEqual(8)
  })

  /// A menu taller than the window must show its TOP — scrolling down to reach
  /// the first entry is worse than not seeing the last one.
  it('keeps the top on screen when the menu cannot fit at all', () => {
    const p = placeMenu(button(400), { width: 220, height: 900 }, VIEW)
    expect(p.top).toBe(8)
    expect(p.left).toBeGreaterThanOrEqual(8)
  })

  it('respects the margin on every edge', () => {
    for (const top of [0, 5, 780, 799]) {
      for (const left of [0, 5, 940, 999]) {
        const p = placeMenu(button(top, left), MENU, VIEW)
        expect(p.top).toBeGreaterThanOrEqual(8)
        expect(p.left).toBeGreaterThanOrEqual(8)
        expect(p.top + MENU.height).toBeLessThanOrEqual(VIEW.height - 8)
        expect(p.left + MENU.width).toBeLessThanOrEqual(VIEW.width - 8)
      }
    }
  })
})
