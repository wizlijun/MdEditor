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
 *
 * Every caller that surfaces the dialog MUST go through this function rather
 * than writing `uiState.showSettings = true` directly — that's what keeps
 * `pendingSettingsTab` paired with the open it was requested for. A caller
 * that bypassed it could leave a stale tab request sitting unconsumed if the
 * dialog closes before SettingsDialog's effect flushes; `closeSettings`
 * clears the flag as a second line of defense against exactly that.
 */
export function openSettings(tab?: string) {
  uiState.showSettings = true
  if (tab) uiState.pendingSettingsTab = tab
}
export function closeSettings() {
  uiState.showSettings = false
  uiState.pendingSettingsTab = null
}
