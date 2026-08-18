import { describe, it, expect } from 'vitest'
import { providerKey, rememberProvider, rememberedProvider } from './types'

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
