import { Store } from '@tauri-apps/plugin-store'
import { t } from '../i18n/store.svelte'
import type { Tab } from '../tabs.svelte'
import {
  type Side, type SideView, type SidePanelState,
  SIDE_BOUNDS, clampWidth, shownViews, resolveActiveView, computeToggle,
  migrateLeft, migrateRight,
} from './model'
import { folderView } from '../folder-view.svelte'
import { outlineGate, isOutlineNoteTab } from '../outline/gate.svelte'
import { historyGate, historyAppliesTo } from '../git-history/gate.svelte'
import { sotvaultStore } from '../sotvault.svelte'

// ---- registry (plain module array; identity is stable across the app) --------
const registered: SideView[] = []

export function registerSideView(v: SideView): void {
  if (registered.some((x) => x.id === v.id)) return
  registered.push(v)
}
export function getSideView(id: string): SideView | undefined {
  return registered.find((v) => v.id === id)
}
export function registeredSideViews(): SideView[] {
  return registered
}

// ---- per-side reactive state -------------------------------------------------
export const sidePanels = $state<{ left: SidePanelState; right: SidePanelState }>({
  left: { visible: false, activeId: null, width: SIDE_BOUNDS.left.default },
  right: { visible: false, activeId: null, width: SIDE_BOUNDS.right.default },
})

// ---- reactive selectors (call the pure model with live registry + state) -----
export function sideShownViews(side: Side, tab: Tab | null): SideView[] {
  return shownViews(registered, side, tab)
}
export function sideActiveView(side: Side, tab: Tab | null): SideView | null {
  return resolveActiveView(sideShownViews(side, tab), sidePanels[side].activeId)
}
export function isSideVisible(side: Side, tab: Tab | null): boolean {
  return sidePanels[side].visible && sideShownViews(side, tab).length > 0
}

// ---- persistence -------------------------------------------------------------
let store: Awaited<ReturnType<typeof Store.load>> | null = null
async function getStore() {
  if (!store) store = await Store.load('settings.json')
  return store
}

// Private helpers: mutate state + s.set(...) but do NOT call s.save().
async function persistVisible(side: Side, v: boolean, s: Awaited<ReturnType<typeof getStore>>) {
  sidePanels[side].visible = v
  await s.set(`sidebar.${side}.visible`, v)
}
async function persistActive(side: Side, id: string, s: Awaited<ReturnType<typeof getStore>>) {
  sidePanels[side].activeId = id
  await s.set(`sidebar.${side}.activeId`, id)
}

export async function setSideVisible(side: Side, v: boolean): Promise<void> {
  const s = await getStore()
  await persistVisible(side, v, s)
  await s.save()
}
export async function setActiveView(side: Side, id: string): Promise<void> {
  const s = await getStore()
  await persistActive(side, id, s)
  await s.save()
}
/** Update width in state only (clamped, no persist). Call during drag. */
export function setSideWidthLive(side: Side, w: number): void {
  sidePanels[side].width = clampWidth(side, w)
}
export async function setSideWidth(side: Side, w: number): Promise<void> {
  const c = clampWidth(side, w)
  sidePanels[side].width = c
  const s = await getStore()
  await s.set(`sidebar.${side}.width`, c)
  await s.save()
}

/** Menu/shortcut toggle: open onto / switch to / hide the given view. */
export async function toggleSideView(id: string): Promise<void> {
  const view = getSideView(id)
  if (!view) return
  const r = computeToggle(sidePanels[view.side], view)
  const s = await getStore()
  await persistActive(view.side, r.activeId, s)
  await persistVisible(view.side, r.visible, s)
  await s.save()
}

/** Hydrate per-side state; migrate from legacy keys on first run. */
export async function loadSidePanels(): Promise<void> {
  const s = await getStore()

  const leftVisible = await s.get<boolean>('sidebar.left.visible')
  if (leftVisible === undefined) {
    const m = migrateLeft({
      visible: await s.get<boolean>('folderView.visible'),
      width: await s.get<number>('folderView.width'),
    })
    sidePanels.left = m
    await s.set('sidebar.left.visible', m.visible)
    await s.set('sidebar.left.activeId', m.activeId)
    await s.set('sidebar.left.width', m.width)
  } else {
    sidePanels.left = {
      visible: leftVisible,
      activeId: (await s.get<string>('sidebar.left.activeId')) ?? 'folder-view',
      width: clampWidth('left', (await s.get<number>('sidebar.left.width')) ?? SIDE_BOUNDS.left.default),
    }
  }

  const rightVisible = await s.get<boolean>('sidebar.right.visible')
  if (rightVisible === undefined) {
    const m = migrateRight({
      outlineVisible: await s.get<boolean>('outline.visible'),
      outlineWidth: await s.get<number>('outline.width'),
      historyVisible: await s.get<boolean>('history.visible'),
      historyWidth: await s.get<number>('history.width'),
    })
    sidePanels.right = m
    await s.set('sidebar.right.visible', m.visible)
    await s.set('sidebar.right.activeId', m.activeId)
    await s.set('sidebar.right.width', m.width)
  } else {
    sidePanels.right = {
      visible: rightVisible,
      activeId: (await s.get<string>('sidebar.right.activeId')) ?? null,
      width: clampWidth('right', (await s.get<number>('sidebar.right.width')) ?? SIDE_BOUNDS.right.default),
    }
  }

  await s.save()
}

/** Register the three built-in views. Idempotent; call once at startup. */
export function registerBuiltinSideViews(): void {
  registerSideView({
    id: 'folder-view', side: 'left', order: 0,
    title: () => t('folderView.tabTitle'),
    isAvailable: () => folderView.enabled,
    appliesTo: () => true,
    component: () => import('../../components/FolderView.svelte'),
  })
  registerSideView({
    id: 'vault-search', side: 'left', order: 20,
    title: () => t('search.title'),
    isAvailable: () => !!sotvaultStore.vaultRoot,
    appliesTo: () => true,
    component: () => import('../../components/side-panel/SearchPanel.svelte'),
  })
  registerSideView({
    id: 'outline-notes', side: 'right', order: 0,
    title: () => t('outline.title'),
    isAvailable: () => outlineGate.enabled,
    appliesTo: (tab) => !(tab != null && isOutlineNoteTab(tab)),
    component: () => import('../../components/outline/OutlinePanel.svelte'),
  })
  registerSideView({
    id: 'git-history', side: 'right', order: 1,
    title: () => t('history.title'),
    isAvailable: () => historyGate.enabled,
    appliesTo: (tab) => tab != null && historyAppliesTo(tab, sotvaultStore.vaultRoot),
    component: () => import('../../components/history/HistoryPanel.svelte'),
  })
}
