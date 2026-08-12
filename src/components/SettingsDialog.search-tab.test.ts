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
// `sotvault_vault_root` is special-cased to resolve — several Save buttons
// in the new C-T11 blocks below (source globs / weights) are correctly
// gated on `vaultSettings.vaultPath` being truthy (same gate the
// pre-existing search-exclude-dirs/threshold Save buttons use), and that
// field is populated from exactly this command in `loadVaultSettings()`.
// Every OTHER command still rejects, including `notemd_vault_settings_set`
// itself — the weights-rejection test below depends on that save call
// failing.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'sotvault_vault_root') return '/tmp/vault'
    throw new Error('no tauri host in vitest')
  }),
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
// The unlabeled-tier-click test (design spec §7.4) routes through
// `setActiveView`/`setSideVisible` (`../lib/side-panel/registry.svelte`),
// both of which call `Store.load(...)` before touching `sidePanels` state —
// without this mock that hits the same unmocked `invoke` boundary as
// everything else here and would reject before `sidePanels`/`searchStore`
// ever update. Same stub shape as `SearchPanel.test.ts`'s.
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: vi.fn(async () => undefined), set: vi.fn(async () => {}), save: vi.fn(async () => {}) })) },
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
import type { SearchStats, SearchProgress, SearchResponse } from '../lib/search/api'
import { searchStore, _setSearchImpl } from '../lib/search/store.svelte'
import { sidePanels } from '../lib/side-panel/registry.svelte'
import { toasts } from '../lib/toast.svelte'

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
    originCounts: { human: 40, derived: 70, source: 18, unlabeled: 9 },
    typeCounts: { 'Book Summary': 25, Answer: 12 },
  }))
  progress = vi.fn(async () => null)
  _setIndexApi({ stats, progress, rebuild: vi.fn(async () => {}) })
  searchStore.clear()
  sidePanels.left.visible = false
  sidePanels.left.activeId = null
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

// Locates the "Provenance tiers" section specifically — not the whole
// dialog body — and reads each row as a (label, value) PAIR rather than a
// bag of substrings. Review round 1 finding: the original version of this
// suite asserted `document.body.textContent` contained each expected number
// and label independently. That passed even when the reviewer swapped
// `oc.human` and `oc.source` under each other's labels in the component —
// an inverted pole is the one failure mode that actively defeats spec §9's
// discovery path ("source looks high → you're missing frontmatter"), and a
// substring check cannot see it because both numbers are still present
// SOMEWHERE on the page, just under the wrong label.
function tierSection(): Element {
  const section = Array.from(document.body.querySelectorAll('section.block')).find(
    (s) => s.querySelector('h3')?.textContent?.trim() === 'Provenance tiers',
  )
  if (!section) throw new Error('Provenance tiers section not found')
  return section
}

function rowValue(section: Element, label: string): string {
  const row = Array.from(section.querySelectorAll(':scope > .row')).find(
    (r) => r.querySelector('.lbl')?.textContent?.trim() === label,
  )
  if (!row) throw new Error(`no row for label "${label}" in tier section`)
  const spans = row.querySelectorAll('span')
  return spans[spans.length - 1]?.textContent?.trim() ?? ''
}

// Task B-T8 (design spec §6/§9): the placeholder a previous project left
// ("Per-tier statistics are coming soon.") is now filled in with real
// numbers read through the same one entry point as the rest of the tab —
// no second load path, per the regression this file exists to prevent.
describe('SettingsDialog — Search & Index tab, per-tier statistics (task B-T8)', () => {
  it('renders each origin/type count next to its own label, not just anywhere on the page', async () => {
    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    // From the beforeEach stub: human 40, derived 70, source 18, unlabeled 9,
    // typeCounts { 'Book Summary': 25, Answer: 12 } → untyped derived
    // remainder is 70 - (25 + 12) = 33. Four distinct tier numbers (40, 70,
    // 18, 9) deliberately chosen so no two tiers could pass by coincidence
    // if their labels were swapped underneath them — see this file's header
    // comment on why `rowValue` locates by label instead of `toContain`.
    const section = tierSection()
    expect(rowValue(section, 'Written by you')).toBe('40') // origin.human pole
    expect(rowValue(section, 'AI-produced')).toBe('70') // origin.derived total
    expect(rowValue(section, 'Book Summary')).toBe('25') // raw concept_type, not translated
    expect(rowValue(section, 'Answer')).toBe('12')
    expect(rowValue(section, 'Other')).toBe('33') // computed untyped-derived remainder
    expect(rowValue(section, 'Raw source material')).toBe('18') // origin.source pole
    expect(rowValue(section, 'Unlabeled')).toBe('9') // origin.unlabeled — task C-T11's fourth tier

    // The old placeholder sentence must be gone.
    expect(document.body.textContent).not.toContain('coming soon')
    unmount(app)
  })

  it('the rendered numbers track the stats payload, not a fixed stub (mutation check)', async () => {
    stats.mockResolvedValue({
      files: 5, blocks: 5, dbBytes: 1, builtAt: null, tokenizerId: 'x',
      skippedLarge: [],
      originCounts: { human: 7, derived: 9, source: 3, unlabeled: 11 },
      typeCounts: { Idea: 4 },
    })
    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = tierSection()
    expect(rowValue(section, 'Written by you')).toBe('7')
    expect(rowValue(section, 'AI-produced')).toBe('9')
    expect(rowValue(section, 'Idea')).toBe('4')
    // 9 - 4 = 5 untyped-derived — proves the "Other" row is recomputed per
    // payload, not a number left over from the default stub (40/70/18/33/9).
    expect(rowValue(section, 'Other')).toBe('5')
    expect(rowValue(section, 'Raw source material')).toBe('3')
    expect(rowValue(section, 'Unlabeled')).toBe('11')
    unmount(app)
  })
})

function namedSection(heading: string): Element {
  const section = Array.from(document.body.querySelectorAll('section.block')).find(
    (s) => s.querySelector('h3')?.textContent?.trim() === heading,
  )
  if (!section) throw new Error(`no section for heading "${heading}"`)
  return section
}

function buttonByText(scope: Element | Document, text: string): HTMLButtonElement {
  const btn = Array.from(scope.querySelectorAll('button')).find((b) => b.textContent?.trim() === text)
  if (!btn) throw new Error(`no button with text "${text}"`)
  return btn as HTMLButtonElement
}

function typeInto(el: Element, value: string): void {
  const input = el as HTMLInputElement
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

// Task C-T10/C-T11 (design spec §7.1): "generate from a sample path" must
// default-select the NARROWEST candidate (the file's own directory) — not
// whichever candidate happens to match the fewest real files. A reviewer
// proved the match-count ladder can invert in an ordinary mixed-media
// import folder, so a UI that defaulted to "smallest count" would silently
// pick the wrong rung there. `suggestGlobs('ebook/三体/book.md')`'s own
// contract (see `glob-suggest.ts`) puts `ebook/三体/**` first.
describe('SettingsDialog — Search & Index tab, source-glob candidates (task C-T11)', () => {
  it('pasting a sample path selects the narrowest candidate by default', async () => {
    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Raw source patterns')
    typeInto(section.querySelector('input[type="text"]')!, 'ebook/三体/book.md')
    await settle()
    buttonByText(section, 'Generate candidates').click()
    await settle()

    const checked = document.body.querySelector('input[name="glob-candidate"]:checked') as HTMLInputElement | null
    expect(checked?.value).toBe('ebook/三体/**')
    unmount(app)
  })
})

// Design spec §7.4: the "Unlabeled" tier row is the designed exit from the
// ×0.3 demotion — it must be a real, clickable action, not decoration.
describe('SettingsDialog — Search & Index tab, unlabeled tier click (design spec §7.4)', () => {
  it('clicking the Unlabeled row runs origin:unlabeled in the search panel and closes the dialog', async () => {
    let lastQuery: string | null = null
    _setSearchImpl(async (q): Promise<SearchResponse> => {
      lastQuery = q
      return { route: 't1-fts', tookMs: 1, total: 0, hits: [], truncated: false, deepAvailable: false }
    })

    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = tierSection()
    const row = Array.from(section.querySelectorAll(':scope > .row')).find(
      (r) => r.querySelector('.lbl')?.textContent?.trim() === 'Unlabeled',
    ) as HTMLElement
    row.click()
    await settle()

    expect(lastQuery).toBe('origin:unlabeled')
    expect(sidePanels.left.visible).toBe(true)
    expect(sidePanels.left.activeId).toBe('vault-search')
    // The dialog is a full-screen overlay over the search panel — it has to
    // close for the results to actually be visible.
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
    unmount(app)
  })
})

// Design spec §8: weight validation happens at the backend command boundary
// and keeps the previously-stored value on rejection — this component must
// reflect that, not keep showing whatever the user just typed as if it took.
describe('SettingsDialog — Search & Index tab, weights save rejection (design spec §8)', () => {
  it('a rejected weight save reverts the draft instead of keeping the typed value', async () => {
    const app = await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Ranking weights')
    const humanInput = section.querySelector('input[type="number"]') as HTMLInputElement
    expect(humanInput.value).toBe('1.25') // the shipped default (design spec §3.1)
    typeInto(humanInput, '0')
    await settle()
    expect(humanInput.value).toBe('0')

    buttonByText(section, 'Save').click()
    await settle()

    // `invoke` is globally mocked to always reject ("no tauri host in
    // vitest") — the same shape a real backend rejection takes (spec §8:
    // zero is invalid, the previous value is retained). The draft must
    // revert to that retained default, not keep displaying the rejected `0`.
    expect(humanInput.value).toBe('1.25')
    expect(toasts.list.at(-1)?.level).toBe('error')
    unmount(app)
  })
})
