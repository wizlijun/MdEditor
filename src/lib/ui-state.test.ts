import { describe, it, expect, beforeEach } from 'vitest'
import { uiState, openSettings, closeSettings, consumePendingSettingsTab } from './ui-state.svelte'

beforeEach(() => {
  uiState.showSettings = false
  uiState.pendingSettingsTab = null
})

describe('openSettings', () => {
  it('opens the dialog without touching pendingSettingsTab when called with no tab', () => {
    uiState.pendingSettingsTab = 'search'  // simulate a stale/previous request
    openSettings()
    expect(uiState.showSettings).toBe(true)
    expect(uiState.pendingSettingsTab).toBe('search')
  })

  // The search panel's gear button needs the dialog to land on 'search' even
  // when it's already open — this is what lets a second click re-select the
  // tab after the user manually navigated away from it.
  it('stashes the requested tab so SettingsDialog can jump to it', () => {
    openSettings('search')
    expect(uiState.showSettings).toBe(true)
    expect(uiState.pendingSettingsTab).toBe('search')
  })

  it('overwrites a previously pending tab with the newly requested one', () => {
    uiState.pendingSettingsTab = 'vault'
    openSettings('search')
    expect(uiState.pendingSettingsTab).toBe('search')
  })
})

describe('closeSettings', () => {
  it('closes the dialog', () => {
    uiState.showSettings = true
    closeSettings()
    expect(uiState.showSettings).toBe(false)
  })

  // `closeSettings` is not on any of SettingsDialog's real close paths (the
  // overlay/Escape/Done handlers all write the bindable `open` prop
  // directly), so this clear is a courtesy for other hypothetical callers,
  // not the mechanism that prevents the hijack — see `consumePendingSettingsTab`
  // below for that. Still correct for callers that do go through it.
  it('clears a pending tab request', () => {
    uiState.showSettings = true
    uiState.pendingSettingsTab = 'search'
    closeSettings()
    expect(uiState.pendingSettingsTab).toBeNull()
  })
})

describe('consumePendingSettingsTab', () => {
  // This is the actual fix for the hijack a reviewer flagged: it's the exact
  // function `SettingsDialog.svelte`'s consuming `$effect` calls, on every
  // change to `pendingSettingsTab`, unconditionally — not gated on whether
  // the dialog happens to be open. A flag that is cleared the moment it's
  // read can never outlive the request that set it, regardless of what the
  // dialog's open/close state is doing around it.
  it('returns the pending tab and clears it in the same call', () => {
    uiState.pendingSettingsTab = 'search'
    const tab = consumePendingSettingsTab()
    expect(tab).toBe('search')
    expect(uiState.pendingSettingsTab).toBeNull()
  })

  it('returns null and is a no-op when nothing is pending', () => {
    const tab = consumePendingSettingsTab()
    expect(tab).toBeNull()
    expect(uiState.pendingSettingsTab).toBeNull()
  })

  // The hijack scenario itself: a tab request consumed once must not still
  // be sitting there for some later, unrelated `openSettings()` call (e.g.
  // a plain Preferences-menu click) to pick up.
  it('a second consume after the first sees nothing left over', () => {
    uiState.pendingSettingsTab = 'search'
    consumePendingSettingsTab()
    openSettings() // no tab argument — the plain Preferences-menu case
    expect(consumePendingSettingsTab()).toBeNull()
  })
})
