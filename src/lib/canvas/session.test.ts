import { beforeEach, describe, expect, it } from 'vitest'
import {
  acquireCanvasUiSession,
  clearCanvasUiSessions,
  markCanvasUiSessionContent,
  releaseCanvasUiSession,
} from './session'
import type { CanvasDocument } from './types'

function doc(text: string): CanvasDocument {
  return {
    nodes: [{
      id: 'n', type: 'text', text, x: 0, y: 0, width: 100, height: 100,
      extras: new Map(), preservedInvalid: new Map(), optionalPresence: new Set(),
    }],
    edges: [], extras: new Map(), presence: { nodes: true, edges: true },
  }
}

describe('CanvasUiSession', () => {
  beforeEach(clearCanvasUiSessions)

  it('keeps history across surface remounts for the same tab content', () => {
    const first = acquireCanvasUiSession('tab', 'v1')
    first.history.record('edit', doc('a'), doc('b'))
    markCanvasUiSessionContent('tab', 'v2')

    const remounted = acquireCanvasUiSession('tab', 'v2')
    expect(remounted).toBe(first)
    expect(remounted.history.canUndo).toBe(true)
  })

  it('clears old history when content was externally replaced', () => {
    const first = acquireCanvasUiSession('tab', 'v1')
    first.history.record('edit', doc('a'), doc('b'))

    const remounted = acquireCanvasUiSession('tab', 'external')
    expect(remounted.history.canUndo).toBe(false)
    expect(remounted.content).toBe('external')
  })

  it('releases closed tabs', () => {
    const first = acquireCanvasUiSession('tab', 'v1')
    releaseCanvasUiSession('tab')
    expect(acquireCanvasUiSession('tab', 'v1')).not.toBe(first)
  })
})
