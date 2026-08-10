import { Store } from '@tauri-apps/plugin-store'
import { DEFAULT_SHORTCUTS, normalizeShortcut, type OutlineCommandId } from './shortcuts'
import { companionPathFor, OUTLINE_SUFFIX_RE } from './store.svelte'
import { platform } from '../platform.svelte'

export const DEFAULT_WIDTH = 360
export const MIN_WIDTH = 240
export const MAX_WIDTH = 640

export const outlineGate = $state<{ enabled: boolean; visible: boolean; width: number }>({
  enabled: false,
  visible: false,
  width: DEFAULT_WIDTH,
})

export const outlineShortcuts = $state<{ overrides: Partial<Record<OutlineCommandId, string>> }>({ overrides: {} })

let store: Awaited<ReturnType<typeof Store.load>> | null = null
async function getStore() {
  if (!store) store = await Store.load('settings.json')
  return store
}

/** Call after settings hydration (same timing as loadFolderViewState). */
export async function loadOutlineGate(): Promise<void> {
  // Core-ized: always enabled; no plugin gate.
  outlineGate.enabled = true
  const s = await getStore()
  outlineGate.visible = (await s.get<boolean>('outline.visible')) ?? false
  outlineGate.width = (await s.get<number>('outline.width')) ?? DEFAULT_WIDTH
  outlineShortcuts.overrides = (await s.get<Partial<Record<OutlineCommandId, string>>>('outline.shortcuts')) ?? {}
}

export async function setShortcutOverride(id: OutlineCommandId, shortcut: string | null): Promise<void> {
  const n = shortcut ? normalizeShortcut(shortcut) : null
  if (n && n !== DEFAULT_SHORTCUTS[id]) outlineShortcuts.overrides[id] = n
  else delete outlineShortcuts.overrides[id]
  const s = await getStore()
  await s.set('outline.shortcuts', { ...outlineShortcuts.overrides })
  await s.save()
}

/** @deprecated visibility/width now live in the side-panel registry (sidePanels.right).
 *  These remain only for settings hydration + test compatibility; the UI no longer reads them. */
export async function setOutlineVisible(v: boolean): Promise<void> {
  outlineGate.visible = v
  const s = await getStore()
  await s.set('outline.visible', v)
  await s.save()
}

/** Update width in state only (clamped, no persist). Call during drag. */
export function setOutlineWidthLive(w: number): void {
  outlineGate.width = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, w))
}

export async function setOutlineWidth(w: number): Promise<void> {
  const clamped = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(w)))
  outlineGate.width = clamped
  const s = await getStore()
  await s.set('outline.width', clamped)
  await s.save()
}

/** Returns true when the outline panel should be shown for a given tab.
 *  Delegates to companionPathFor so "applicable" and "has a companion path"
 *  can never disagree (e.g. on `.NOTES.MD` case variants). */
export function outlineAppliesTo(tab: { kind: string; filePath: string }): boolean {
  return tab.kind === 'markdown' && companionPathFor(tab.filePath) != null
}

let isIos = false
void platform().then((p) => { isIos = p === 'ios' }).catch(() => {})

/** 全屏大纲 tab gate:桌面端 + .note.md/.notes.md 后缀(spec §3)。
 *  outlineGate.enabled 是 $state(core-ized 后恒为 true),在组件 $derived 中调用。 */
export function isOutlineNoteTab(tab: { kind: string; filePath: string }): boolean {
  return !isIos && outlineGate.enabled && tab.kind === 'markdown' && OUTLINE_SUFFIX_RE.test(tab.filePath)
}
