// @vitest-environment happy-dom
//
// The regression this file exists for: the Search & Index tab used to load
// its data from the tab-strip button's `onclick`, so the OTHER entry point —
// `SearchPanel.svelte`'s gear button, which goes through
// `openSettings('search')` → `pendingSettingsTab` → `selectedTab` and never
// touches that button — opened a page that had never called `stats()`. A
// fully built index rendered as "— / — / Last built: Never" plus the
// affirmative falsehood "No files are currently skipped", on the very panel
// whose job is to explain why a file is missing from search.
//
// Three separate task reviews accepted the claim that no component-test
// harness existed for this file. `ThemeImportDialog.test.ts` is one; the real
// obstacle was only SettingsDialog's import surface, mocked below.
import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

// SettingsDialog pulls in most of the app's store layer transitively. Only
// the Tauri host boundary needs stubbing — the stores themselves run for
// real, which is what makes this a test of the wiring rather than of a mock.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => { throw new Error('no tauri host in vitest') }),
  convertFileSrc: (p: string) => p,
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({
  ask: vi.fn(async () => false),
  confirm: vi.fn(async () => false),
  open: vi.fn(async () => null),
  save: vi.fn(async () => null),
  message: vi.fn(async () => {}),
}))
// Desktop, not iOS — the tab is `!isIOSPlatform`-gated in the strip.
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))
vi.mock('@tauri-apps/plugin-updater', () => ({ check: vi.fn(async () => null) }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: vi.fn(async () => {}), exit: vi.fn(async () => {}) }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  WebviewWindow: class { static getCurrent() { return { label: 'main' } } },
  getCurrentWebviewWindow: () => ({ label: 'main', listen: async () => () => {} }),
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ label: 'main', listen: async () => () => {} }),
}))

import { openSettings, closeSettings } from '../lib/ui-state.svelte'
import { indexStatus, _setIndexApi } from '../lib/search/index-status.svelte'
import { sotvaultStore } from '../lib/sotvault.svelte'
import type { SearchStats, SearchProgress } from '../lib/search/api'

let stats: Mock<() => Promise<SearchStats | null>>
let progress: Mock<() => Promise<SearchProgress | null>>

beforeEach(() => {
  // See ThemeImportDialog.test.ts: resetModules() breaks Svelte's DOM
  // operations singleton under happy-dom; clearAllMocks is enough.
  vi.clearAllMocks()
  document.body.innerHTML = ''
  // The tab's body is gated on having a vault at all ("Open a vault to see
  // its search index status") — without this the render assertions would be
  // checking the empty-state branch.
  sotvaultStore.vaultRoot = '/tmp/vault'
  indexStatus.reset()
  stats = vi.fn(async () => ({
    files: 128, blocks: 900, dbBytes: 4096, builtAt: '2026-08-11T00:00:00Z',
    tokenizerId: 'jieba-v1', skippedLarge: [{ path: 'big.md', sizeBytes: 9_000_000 }],
  }))
  progress = vi.fn(async () => null)
  _setIndexApi({ stats, progress, rebuild: vi.fn(async () => {}) })
})

afterEach(() => { closeSettings(); sotvaultStore.vaultRoot = null })

async function mountDialog() {
  const { default: SettingsDialog } = await import('./SettingsDialog.svelte')
  return mount(SettingsDialog as unknown as Parameters<typeof mount>[0], {
    target: document.body,
    props: { open: true },
  })
}

// Lets the pending-tab $effect, the tab-entry $effect and the async
// stats()/progress() promises all settle.
async function settle() {
  flushSync()
  for (let i = 0; i < 5; i++) await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  flushSync()
}

describe('SettingsDialog — Search & Index tab entry', () => {
  it('gear-button path (openSettings("search")) actually loads the index status', async () => {
    const app = await mountDialog()
    await settle()
    expect(stats).not.toHaveBeenCalled() // 'core' tab: nothing search-related yet

    openSettings('search') // exactly what SearchPanel's gear button calls
    await settle()

    expect(stats).toHaveBeenCalled()
    expect(indexStatus.stats?.files).toBe(128)
    unmount(app)
  })

  it('the same path also renders the loaded numbers instead of the em-dash placeholders', async () => {
    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const text = document.body.textContent ?? ''
    expect(text).toContain('128')       // file count, not '—'
    expect(text).toContain('jieba-v1')  // tokenizer id, not '—'
    // The skipped list is populated, so the "nothing is skipped" reassurance
    // must NOT be on screen — that sentence being a lie is the whole bug.
    expect(text).toContain('big.md')
    expect(text).not.toContain('No files are currently skipped')
    unmount(app)
  })

  it('the tab-strip button path loads it too — one entry point, not two', async () => {
    const app = await mountDialog()
    await settle()

    const btn = Array.from(document.body.querySelectorAll('nav.tab-strip button'))
      .find((b) => b.textContent?.trim() === 'Search & Index') as HTMLButtonElement
    expect(btn, 'the Search & Index tab button should be in the strip').toBeTruthy()
    btn.click()
    await settle()

    expect(stats).toHaveBeenCalled()
    unmount(app)
  })
})
