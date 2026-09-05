import { describe, it, expect, vi, beforeEach } from 'vitest'

const storeGet = vi.fn()
const storeSet = vi.fn()
const storeSave = vi.fn()
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: storeGet, set: storeSet, save: storeSave })) },
}))
// The gate/store modules pulled in by registerBuiltinSideViews are not exercised
// here; stub them so importing the registry stays side-effect free.
vi.mock('../folder-view.svelte', () => ({ folderView: { enabled: true } }))
vi.mock('../outline/gate.svelte', () => ({ outlineGate: { enabled: true }, isOutlineNoteTab: () => false }))
vi.mock('../git-history/gate.svelte', () => ({ historyGate: { enabled: true }, historyAppliesTo: () => true }))
vi.mock('../sotvault.svelte', () => ({ sotvaultStore: { vaultRoot: '/vault' } }))
vi.mock('../i18n/store.svelte', () => ({ t: (k: string) => k }))

import {
  sidePanels, registerSideView, getSideView, loadSidePanels, toggleSideView,
  registerBuiltinSideViews,
} from './registry.svelte'
import type { SideView } from './model'

function stub(id: string, side: 'left' | 'right'): SideView {
  return {
    id, side, order: 0, title: () => id,
    isAvailable: () => true, appliesTo: () => true,
    component: async () => ({ default: null as never }),
  }
}

beforeEach(() => {
  storeGet.mockReset(); storeSet.mockReset(); storeSave.mockReset()
  sidePanels.left = { visible: false, activeId: null, width: 240 }
  sidePanels.right = { visible: false, activeId: null, width: 360 }
})

describe('loadSidePanels migration', () => {
  it('derives new sidebar.* state from legacy keys when absent', async () => {
    storeGet.mockImplementation(async (k: string) => ({
      'folderView.visible': true, 'folderView.width': 300,
      'outline.visible': true, 'outline.width': 400,
    } as Record<string, unknown>)[k])
    await loadSidePanels()
    expect(sidePanels.left).toEqual({ visible: true, activeId: 'folder-view', width: 300 })
    expect(sidePanels.right).toEqual({ visible: true, activeId: 'outline-notes', width: 400 })
    expect(storeSet).toHaveBeenCalledWith('sidebar.left.activeId', 'folder-view')
    expect(storeSet).toHaveBeenCalledWith('sidebar.right.activeId', 'outline-notes')
  })

  it('reads new keys directly when present', async () => {
    storeGet.mockImplementation(async (k: string) => ({
      'sidebar.left.visible': false, 'sidebar.left.activeId': 'folder-view', 'sidebar.left.width': 260,
      'sidebar.right.visible': true, 'sidebar.right.activeId': 'git-history', 'sidebar.right.width': 500,
    } as Record<string, unknown>)[k])
    await loadSidePanels()
    expect(sidePanels.left.width).toBe(260)
    expect(sidePanels.right).toEqual({ visible: true, activeId: 'git-history', width: 500 })
  })
})

describe('built-in views', () => {
  it('registers the table of contents beside sidecar notes and history', () => {
    registerBuiltinSideViews()
    expect(getSideView('outline-notes')).toMatchObject({ side: 'right', order: 0 })
    expect(getSideView('table-of-contents')).toMatchObject({ side: 'right', order: 1 })
    expect(getSideView('table-of-contents')?.title()).toBe('toc.title')
    expect(getSideView('git-history')).toMatchObject({ side: 'right', order: 2 })
  })
})

describe('toggleSideView', () => {
  beforeEach(() => {
    // registry is module-global; ensure our stubs exist (idempotent).
    registerSideView(stub('outline-notes', 'right'))
    registerSideView(stub('git-history', 'right'))
    storeGet.mockResolvedValue(undefined)
  })
  it('opens the side onto the toggled view', async () => {
    expect(getSideView('outline-notes')).toBeDefined()
    await toggleSideView('outline-notes')
    expect(sidePanels.right.visible).toBe(true)
    expect(sidePanels.right.activeId).toBe('outline-notes')
  })
  it('switches tab, then hides on repeat toggle of the active view', async () => {
    await toggleSideView('outline-notes')
    await toggleSideView('git-history')
    expect(sidePanels.right).toMatchObject({ visible: true, activeId: 'git-history' })
    await toggleSideView('git-history')
    expect(sidePanels.right.visible).toBe(false)
  })
  it('does not throw and leaves state unchanged for unknown id', async () => {
    const leftBefore = { ...sidePanels.left }
    const rightBefore = { ...sidePanels.right }
    await expect(toggleSideView('nonexistent-id')).resolves.toBeUndefined()
    expect(sidePanels.left).toEqual(leftBefore)
    expect(sidePanels.right).toEqual(rightBefore)
  })
})
