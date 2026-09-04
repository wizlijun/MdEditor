import { beforeEach, describe, expect, it } from 'vitest'
import { clearCanvasClipboard, recallCanvasClipboard, rememberCanvasClipboard } from './clipboard'

describe('Canvas in-process clipboard', () => {
  beforeEach(clearCanvasClipboard)

  it('survives surface remounts but never shadows newer system text', () => {
    const payload = { version: 1 as const, nodes: [], edges: [], sourceRoot: '/vault' }
    rememberCanvasClipboard(payload, '{"nodes":[],"edges":[]}')

    expect(recallCanvasClipboard()).toBe(payload)
    expect(recallCanvasClipboard('{"nodes":[],"edges":[]}')).toBe(payload)
    expect(recallCanvasClipboard('new text from another app')).toBeNull()
  })
})
