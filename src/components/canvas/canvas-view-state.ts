import { Store } from '@tauri-apps/plugin-store'

export interface CanvasViewportState {
  x: number
  y: number
  zoom: number
  updatedAt: number
}

const STORE_FILE = 'canvas-view-state-v1.json'
let storePromise: Promise<Store> | null = null

function getStore(): Promise<Store> {
  storePromise ??= Store.load(STORE_FILE)
  return storePromise
}

function valid(value: unknown): value is CanvasViewportState {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Partial<CanvasViewportState>
  return Number.isFinite(candidate.x)
    && Number.isFinite(candidate.y)
    && Number.isFinite(candidate.zoom)
    && Number.isFinite(candidate.updatedAt)
    && candidate.zoom! >= 0.05
    && candidate.zoom! <= 8
}

export async function loadCanvasViewport(path: string): Promise<CanvasViewportState | null> {
  if (!path) return null
  try {
    const value = await (await getStore()).get<unknown>(path)
    return valid(value) ? value : null
  } catch (error) {
    console.warn('[canvas-view-state] load failed:', error)
    return null
  }
}

export async function saveCanvasViewport(path: string, viewport: Omit<CanvasViewportState, 'updatedAt'>): Promise<void> {
  if (!path || !Number.isFinite(viewport.x) || !Number.isFinite(viewport.y) || !Number.isFinite(viewport.zoom)) return
  try {
    const store = await getStore()
    await store.set(path, { ...viewport, updatedAt: Date.now() } satisfies CanvasViewportState)
    await store.save()
  } catch (error) {
    console.warn('[canvas-view-state] save failed:', error)
  }
}

export async function copyCanvasViewport(oldPath: string, newPath: string, removeOld = false): Promise<void> {
  if (!oldPath || !newPath || oldPath === newPath) return
  try {
    const store = await getStore()
    const value = await store.get<unknown>(oldPath)
    if (!valid(value)) return
    await store.set(newPath, { ...value, updatedAt: Date.now() } satisfies CanvasViewportState)
    if (removeOld) await store.delete(oldPath)
    await store.save()
  } catch (error) {
    console.warn('[canvas-view-state] copy failed:', error)
  }
}
