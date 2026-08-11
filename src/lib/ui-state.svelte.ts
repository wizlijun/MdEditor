/**
 * App-level UI state shared between App.svelte and command modules.
 * Hoisted so commands.ts can dispatch 'preferences' without prop-drilling.
 */
export const uiState = $state<{ showSettings: boolean; pendingSettingsTab: string | null }>({
  showSettings: false,
  pendingSettingsTab: null,
})

/**
 * Open the Preferences dialog. With no argument this just surfaces whatever
 * tab was last selected (the dialog stays mounted across opens, so
 * `selectedTab` persists on its own). Pass `tab` to jump to a specific tab —
 * e.g. the search panel's gear button lands on 'search' — even if the
 * dialog is already open, since SettingsDialog reacts to
 * `pendingSettingsTab` changing, not to `open` changing.
 */
export function openSettings(tab?: string) {
  uiState.showSettings = true
  if (tab) uiState.pendingSettingsTab = tab
}

/**
 * NOT on the dialog's hot close path — `SettingsDialog.svelte`'s overlay
 * click / Escape / Done button all write its bindable `open` prop directly,
 * which flows back to `uiState.showSettings` through `bind:open` in
 * `App.svelte` without ever calling this. Kept for callers that hold a
 * reference to `uiState` but not the dialog instance; the
 * `pendingSettingsTab` clear here is a courtesy for those, not the mechanism
 * that keeps the flag from leaking (see `consumePendingSettingsTab` below
 * for that).
 */
export function closeSettings() {
  uiState.showSettings = false
  uiState.pendingSettingsTab = null
}

/**
 * The one and only place `pendingSettingsTab` is read and cleared —
 * `SettingsDialog.svelte`'s consuming `$effect` calls this directly, every
 * time the flag changes, regardless of whether the dialog is currently
 * `open`. That "regardless of open" part is load-bearing: an earlier version
 * gated consumption on `open`, which meant a request set while the dialog
 * was (about to be) closed could survive to silently redirect a later,
 * unrelated `openSettings()` call — the search panel's gear button hijacking
 * a plain Preferences-menu open, for instance. Consuming unconditionally
 * means the flag's lifetime is at most one reactive flush after it's set,
 * open or not, so there is no window in which a stale request can outlive
 * the request that made it.
 */
export function consumePendingSettingsTab(): string | null {
  const tab = uiState.pendingSettingsTab
  uiState.pendingSettingsTab = null
  return tab
}
