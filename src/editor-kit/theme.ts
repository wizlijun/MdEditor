// Theme for the Editor Kit.
//
// The host compiles the user's editor theme and serves it over the bridge
// (`host.theme.css`, spec §3.4 — implemented by its own task). The kit keeps a
// single <style data-kit-theme> slot in the plugin window's head and rewrites it
// whenever the theme or the OS colour scheme changes, so a plugin editor follows
// the user's real theme instead of a degraded light/dark binary.
//
// Every failure path resolves to an empty slot: an unavailable or
// capability-denied theme must not stop the editor from mounting.

interface ThemeCss {
  light_css?: string
  dark_css?: string
  follow_system?: boolean
}

let currentTheme: ThemeCss | null = null
// Every request/push claims a revision. A host push can arrive while the
// initial disk-backed RPC is pending; the older response must never overwrite
// that authoritative in-memory bundle when it eventually resolves.
let themeRevision = 0

function applyThemeCss(t: ThemeCss): void {
  currentTheme = t
  const dark = !!t.follow_system && window.matchMedia('(prefers-color-scheme: dark)').matches
  themeSlot().textContent = (dark ? t.dark_css : t.light_css) ?? ''
}

function themeSlot(): HTMLStyleElement {
  let slot = document.querySelector('style[data-kit-theme]') as HTMLStyleElement | null
  if (!slot) {
    slot = document.createElement('style')
    slot.setAttribute('data-kit-theme', '')
    document.head.appendChild(slot)
  }
  return slot
}

export async function applyKitTheme(theme?: ThemeCss): Promise<void> {
  const revision = ++themeRevision
  if (theme) {
    applyThemeCss(theme)
    return
  }
  const notemd = (window as unknown as { notemd?: { request(m: string, p?: unknown): Promise<ThemeCss> } }).notemd
  const slot = themeSlot()
  try {
    const t = await notemd!.request('host.theme.css', {})
    if (revision !== themeRevision) return
    applyThemeCss(t)
  } catch {
    if (revision !== themeRevision) return
    currentTheme = null
    slot.textContent = ''
  }
}

/**
 * Idempotency guard. The theme slot is window-level state, and the bridge's
 * `onMessage` is push-only with no way to unsubscribe
 * (`plugin_runtime/windows.rs`: `onMessage(cb) { __listeners.push(cb) }`), so a
 * mount → destroy → mount cycle would otherwise stack listeners and fire one
 * RPC per past mount on every theme change. One registration per window is
 * both necessary and sufficient — hence nothing to unregister on destroy.
 */
let watched = false

export function watchKitTheme(): void {
  if (watched) return
  watched = true
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (currentTheme) applyThemeCss(currentTheme)
    else void applyKitTheme()
  })
  const notemd = (window as unknown as { notemd?: { onMessage?(cb: (p: unknown) => void): void } }).notemd
  notemd?.onMessage?.((p: unknown) => {
    const message = p as { type?: string; theme?: ThemeCss } | null
    if (message?.type === 'theme-changed') void applyKitTheme(message.theme)
  })
}
