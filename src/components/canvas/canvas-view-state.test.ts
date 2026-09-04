import { beforeEach, describe, expect, it, vi } from 'vitest'
import { copyCanvasViewport } from './canvas-view-state'

const state = vi.hoisted(() => ({
  values: new Map<string, unknown>(),
  set: vi.fn(async (key: string, value: unknown) => { state.values.set(key, value) }),
  delete: vi.fn(async (key: string) => state.values.delete(key)),
  save: vi.fn(async () => {}),
}))

vi.mock('@tauri-apps/plugin-store', () => ({
  Store: {
    load: vi.fn(async () => ({
      get: async (key: string) => state.values.get(key),
      set: state.set,
      delete: state.delete,
      save: state.save,
    })),
  },
}))

describe('Canvas viewport identity changes', () => {
  beforeEach(() => {
    state.values.clear()
    vi.clearAllMocks()
  })

  it('copies on Save As and migrates on rename', async () => {
    state.values.set('/old.canvas', { x: 10, y: 20, zoom: 1.5, updatedAt: 1 })
    await copyCanvasViewport('/old.canvas', '/copy.canvas')
    expect(state.values.get('/old.canvas')).toBeTruthy()
    expect(state.values.get('/copy.canvas')).toMatchObject({ x: 10, y: 20, zoom: 1.5 })

    await copyCanvasViewport('/copy.canvas', '/renamed.canvas', true)
    expect(state.values.has('/copy.canvas')).toBe(false)
    expect(state.values.get('/renamed.canvas')).toMatchObject({ x: 10, y: 20, zoom: 1.5 })
  })
})
