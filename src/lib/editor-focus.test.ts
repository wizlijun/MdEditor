import { describe, it, expect, vi, afterEach } from 'vitest'

afterEach(() => {
  delete (globalThis as Record<string, unknown>).window
})

describe('editor focus request', () => {
  it('stores a one-shot pending request and dispatches an immediate focus event', async () => {
    const dispatched: CustomEvent[] = []
    ;(globalThis as Record<string, unknown>).window = {
      dispatchEvent: vi.fn((event: CustomEvent) => {
        dispatched.push(event)
        return true
      }),
    }

    const { requestEditorFocus, consumeEditorFocus } = await import('./editor-focus.svelte')
    requestEditorFocus('/tmp/quick.md')

    expect(dispatched).toHaveLength(1)
    expect(dispatched[0].type).toBe('mdeditor:focus-editor')
    expect(dispatched[0].detail).toEqual({ path: '/tmp/quick.md' })
    expect(consumeEditorFocus('/tmp/quick.md')).toBe(true)
    expect(consumeEditorFocus('/tmp/quick.md')).toBe(false)
  })
})
