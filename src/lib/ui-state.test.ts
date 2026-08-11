import { describe, it, expect, beforeEach } from 'vitest'
import { uiState, openSettings, closeSettings } from './ui-state.svelte'

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
})
