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
//
// `defaultInvokeImpl` is exposed via `vi.hoisted` (not just captured in the
// factory closure) so the per-row-match-count test below can temporarily
// swap in a richer implementation (one that also answers
// `notemd_vault_settings_get`/`notemd_search_glob_matches`) and this file's
// `beforeEach` can then restore the plain default for every other test —
// `vi.clearAllMocks()` alone does NOT undo a `.mockImplementation()`
// override, only clears call history.
const { invokeMock, defaultInvokeImpl } = vi.hoisted(() => {
  const defaultInvokeImpl = async (cmd: string, _args?: unknown): Promise<unknown> => {
    if (cmd === 'sotvault_vault_root') return '/tmp/vault'
    throw new Error('no tauri host in vitest')
  }
  return { invokeMock: vi.fn(defaultInvokeImpl), defaultInvokeImpl }
})
vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
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
import { getPluginScopedAll, settings } from '../lib/settings.svelte'
import { DEFAULT_SMART_LOOKUP_SETTINGS } from '../lib/smart-search/settings'
import { ask } from '@tauri-apps/plugin-dialog'

let stats: Mock<() => Promise<SearchStats | null>>
let progress: Mock<() => Promise<SearchProgress | null>>

beforeEach(() => {
  // See ThemeImportDialog.test.ts: resetModules() breaks Svelte's DOM
  // operations singleton under happy-dom; clearAllMocks is enough.
  vi.clearAllMocks()
  invokeMock.mockImplementation(defaultInvokeImpl)
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
    attentionFiles: 0,
    attentionAsOf: null,
  }))
  progress = vi.fn(async () => null)
  _setIndexApi({ stats, progress, rebuild: vi.fn(async () => {}) })
  searchStore.clear()
  sidePanels.left.visible = false
  sidePanels.left.activeId = null
  settings.smartLookup = structuredClone(DEFAULT_SMART_LOOKUP_SETTINGS)
})

// Review round 1, Minor 7: a test that throws BEFORE reaching its own
// `unmount(app)` used to leave a live component instance mounted — still
// subscribed to module-singleton stores like `uiState.pendingSettingsTab` —
// which could then steal that flag from whichever test ran next, turning
// its failure into a misleading "section not found" instead of the real
// assertion mismatch (observed first-hand during this task's own mutation
// checks). Tracking the current instance here and force-unmounting it in
// `afterEach` makes each test's own guarantee independent of whether the
// PREVIOUS test passed, failed cleanly, or threw. Individual tests no
// longer need (or call) `unmount(app)` themselves.
let currentApp: ReturnType<typeof mount> | null = null

afterEach(() => {
  closeSettings()
  sotvaultStore.vaultRoot = null
  if (currentApp) {
    try { unmount(currentApp) } catch { /* already unmounted by the test itself */ }
    currentApp = null
  }
})

async function mountDialog() {
  const { default: SettingsDialog } = await import('./SettingsDialog.svelte')
  currentApp = mount(SettingsDialog as unknown as Parameters<typeof mount>[0], {
    target: document.body,
    props: { open: true },
  })
  return currentApp
}

// Lets the pending-tab $effect, the tab-entry $effect and the async
// stats()/progress() promises all settle.
async function settle() {
  flushSync()
  for (let i = 0; i < 5; i++) await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  flushSync()
}

describe('SettingsDialog — plugin number fields', () => {
  it('renders numeric constraints and only persists finite valueAsNumber values', async () => {
    invokeMock.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'get_plugin_manifests') {
        return [{
          id: 'number-field-test',
          name: 'Number field test',
          version: '1.0.0',
          binary: '',
          host_capabilities: ['settings'],
          settings: {
            tab_label: 'WIP test',
            schema: [{
              key: 'number-field-test.wipLimit',
              type: 'number',
              label: 'WIP limit',
              default: 5,
              min: 1,
              max: 20,
              step: 1,
            }],
          },
        }]
      }
      return defaultInvokeImpl(cmd, args)
    })

    await mountDialog()
    await settle()

    const tab = Array.from(document.body.querySelectorAll('nav.tab-strip button'))
      .find((button) => button.textContent?.trim() === 'WIP test') as HTMLButtonElement
    tab.click()
    await settle()

    const input = document.body.querySelector('.plugin-field input[type="number"]') as HTMLInputElement
    expect(input.value).toBe('5')
    expect(input.min).toBe('1')
    expect(input.max).toBe('20')
    expect(input.step).toBe('1')

    input.value = '7'
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await settle()
    expect(getPluginScopedAll('number-field-test')).toEqual({
      'number-field-test.wipLimit': 7,
    })

    input.value = ''
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await settle()
    expect(getPluginScopedAll('number-field-test')).toEqual({
      'number-field-test.wipLimit': 7,
    })

    input.value = '0'
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await settle()
    expect(getPluginScopedAll('number-field-test')).toEqual({
      'number-field-test.wipLimit': 7,
    })

    input.value = 'not-a-number'
    input.dispatchEvent(new Event('change', { bubbles: true }))
    await settle()
    expect(getPluginScopedAll('number-field-test')).toEqual({
      'number-field-test.wipLimit': 7,
    })
  })
})

describe('SettingsDialog — Search & Index tab entry', () => {
  it('gear-button path (openSettings("search")) actually loads the index status', async () => {
    await mountDialog()
    await settle()
    expect(stats).not.toHaveBeenCalled() // 'core' tab: nothing search-related yet

    openSettings('search') // exactly what SearchPanel's gear button calls
    await settle()

    expect(stats).toHaveBeenCalled()
    expect(indexStatus.stats?.files).toBe(128)
  })

  it('the same path also renders the loaded numbers instead of the em-dash placeholders', async () => {
    await mountDialog()
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
  })

  it('the tab-strip button path loads it too — one entry point, not two', async () => {
    await mountDialog()
    await settle()

    const btn = Array.from(document.body.querySelectorAll('nav.tab-strip button'))
      .find((b) => b.textContent?.trim() === 'Search & Index') as HTMLButtonElement
    expect(btn, 'the Search & Index tab button should be in the strip').toBeTruthy()
    btn.click()
    await settle()

    expect(stats).toHaveBeenCalled()
  })

  it('exposes the persistent Smart Lookup controls on the Search & Index page', async () => {
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = Array.from(document.body.querySelectorAll('section.block')).find(
      (candidate) => candidate.querySelector('h3')?.textContent?.trim() === 'Smart Lookup',
    )
    expect(section).toBeTruthy()
    expect(section?.textContent).toContain('Smart understanding')
    expect(section?.textContent).toContain('Result limit')
    expect(section?.textContent).toContain('Quick summary')
    expect(section?.textContent).toContain('Default handoff Agent')

    const deepRow = Array.from(section!.querySelectorAll('label.row')).find(
      (row) => row.querySelector('.lbl')?.textContent?.trim() === 'Expand automatically when empty',
    )
    const checkbox = deepRow?.querySelector<HTMLInputElement>('input[type="checkbox"]')
    expect(checkbox?.checked).toBe(false)
    checkbox!.checked = true
    checkbox!.dispatchEvent(new Event('change', { bubbles: true }))
    await settle()
    expect(settings.smartLookup.results.autoDeepOnZero).toBe(true)
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
    await mountDialog()
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
  })

  it('the rendered numbers track the stats payload, not a fixed stub (mutation check)', async () => {
    stats.mockResolvedValue({
      files: 5, blocks: 5, dbBytes: 1, builtAt: null, tokenizerId: 'x',
      skippedLarge: [],
      originCounts: { human: 7, derived: 9, source: 3, unlabeled: 11 },
      typeCounts: { Idea: 4 },
      attentionFiles: 0,
      attentionAsOf: null,
    })
    await mountDialog()
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
  })
})

// Task 12 (attention-weighted retrieval spec): ingestion "just not having
// run" produces no visible symptom anywhere else — search silently degrades
// to unweighted results. This row is the only place that fact surfaces, so
// its three states (never run / ran with zero rows / ran with data) must be
// told apart rather than collapsed into "has a number or doesn't".
describe('SettingsDialog — Search & Index tab, attention coverage row (task 12)', () => {
  it('shows N / M once ingestion has run and produced rows', async () => {
    stats.mockResolvedValue({
      files: 100, blocks: 900, dbBytes: 4096, builtAt: '2026-08-11T00:00:00Z',
      tokenizerId: 'jieba-v1', skippedLarge: [],
      originCounts: { human: 40, derived: 70, source: 18, unlabeled: 9 },
      typeCounts: {},
      attentionFiles: 37,
      attentionAsOf: '2026-08-13',
    })
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    expect(screen_getByText('37 / 100')).toBeTruthy()
  })

  it('hides the row entirely when ingestion has never run on this index (attentionAsOf === null)', async () => {
    stats.mockResolvedValue({
      files: 100, blocks: 900, dbBytes: 4096, builtAt: '2026-08-11T00:00:00Z',
      tokenizerId: 'jieba-v1', skippedLarge: [],
      originCounts: { human: 40, derived: 70, source: 18, unlabeled: 9 },
      typeCounts: {},
      attentionFiles: 0,
      attentionAsOf: null,
    })
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    expect(queryByAttentionText()).toBeNull()
  })

  it('shows 0 / M when ingestion ran but produced zero rows — the most important diagnostic case, distinct from never-run', async () => {
    stats.mockResolvedValue({
      files: 100, blocks: 900, dbBytes: 4096, builtAt: '2026-08-11T00:00:00Z',
      tokenizerId: 'jieba-v1', skippedLarge: [],
      originCounts: { human: 40, derived: 70, source: 18, unlabeled: 9 },
      typeCounts: {},
      attentionFiles: 0,
      attentionAsOf: '2026-08-13',
    })
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    expect(screen_getByText('0 / 100')).toBeTruthy()
  })
})

// Small local helpers mirroring @testing-library/dom's semantics without
// pulling in the dependency: the rest of this file already reads the DOM
// directly (see `rowValue`/`tierSection` above), so these two match that
// convention instead of introducing a new import surface.
function screen_getByText(text: string): Element {
  const el = Array.from(document.body.querySelectorAll('span')).find((s) => s.textContent?.trim() === text)
  if (!el) throw new Error(`no element with text "${text}"`)
  return el
}

function queryByAttentionText(): Element | null {
  return Array.from(document.body.querySelectorAll('span.lbl')).find((s) =>
    /注意力|Attention/i.test(s.textContent ?? ''),
  ) ?? null
}

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
    await mountDialog()
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
  })
})

// Review round 1, Important 2: the candidate ladder already showed a count
// per candidate; the SAVED pattern list — what actually ships into the
// index — did not, so a pattern that quietly matches tens of thousands of
// files had no on-screen signal at all (the zero-match warning only fires
// at exactly 0). Every already-saved row must show its real count too.
describe('SettingsDialog — Search & Index tab, saved pattern row shows its real match count (review round 1, Important 2)', () => {
  it('renders the count next to each already-saved pattern, not just the candidate ladder', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'sotvault_vault_root') return '/tmp/vault'
      if (cmd === 'notemd_vault_settings_get') return { searchSourceGlobs: ['ebook/**'] }
      if (cmd === 'notemd_search_glob_matches') return 42
      throw new Error('no tauri host in vitest')
    })

    await mountDialog()
    await settle()
    openSettings('search')
    await settle()
    await settle() // one more tick for the count fetch `refreshGlobCounts` kicks off on load

    const section = namedSection('Raw source patterns')
    const row = section.querySelector('.glob-row')
    const input = row?.querySelector('input[type="text"]') as HTMLInputElement | null
    expect(input?.value).toBe('ebook/**')
    expect(row?.querySelector('.glob-count')?.textContent?.trim()).toBe('42 files')
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

    await mountDialog()
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
  })
})

// Design spec §8: an invalid weight must be rejected and the previously
// stored value retained. Review round 1, Minor 6: Svelte's number-input
// binding writes `null` for a cleared/unparseable field, and the backend
// stores `search_weights` as one wholesale struct (`vault_settings.rs`'s
// `merge` does `out.search_weights = Some(w)`, not a per-field merge) — so
// letting a `null` field slip through used to silently replace the ENTIRE
// stored weights (including any other customized field) with defaults,
// behind a plain "Saved" success toast. The fix intercepts this
// client-side, before any request is sent — the two tests below cover both
// that new path and the pre-existing backend-rejection backstop.
describe('SettingsDialog — Search & Index tab, invalid weight rejected (design spec §8, review round 1 Minor 6)', () => {
  it('an out-of-range value is rejected client-side — no request sent, typed value stays visible, no toast', async () => {
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Ranking weights')
    const humanInput = section.querySelector('input[type="number"]') as HTMLInputElement
    expect(humanInput.value).toBe('1.25') // the shipped default (design spec §3.1)
    typeInto(humanInput, '0')
    await settle()
    expect(humanInput.value).toBe('0')

    const toastCountBefore = toasts.list.length
    buttonByText(section, 'Save').click()
    await settle()

    // Rejected before ever reaching `invoke` — the input keeps showing the
    // value the user typed (neither silently reset to the default nor
    // silently accepted), an inline error names the offending field, and no
    // save toast fires either way because no request was ever made.
    expect(humanInput.value).toBe('0')
    expect(section.querySelector('.result.fail')?.textContent).toContain('Written by you')
    expect(toasts.list.length).toBe(toastCountBefore)
  })

  it('a genuine backend rejection still reverts the draft and shows an error toast (backstop path)', async () => {
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Ranking weights')
    const humanInput = section.querySelector('input[type="number"]') as HTMLInputElement
    typeInto(humanInput, '2') // passes client-side validation (finite, >0, <=5)
    await settle()
    expect(humanInput.value).toBe('2')

    buttonByText(section, 'Save').click()
    await settle()

    // `invoke` is globally mocked to always reject ("no tauri host in
    // vitest") — client-side validation cannot catch every failure (e.g. a
    // concurrent write elsewhere), so this backstop must still revert to
    // the retained previous value and surface an error, per spec §8.
    expect(humanInput.value).toBe('1.25')
    expect(toasts.list.at(-1)?.level).toBe('error')
  })
})

// Review round 2: the round-1 report disclosed Important 3's confirm path
// as "not testable" because `ask` is mocked to always resolve `false`
// file-wide — that reasoning doesn't hold. `ask` is the same
// `vi.fn(async () => false)` shape `invokeMock` is (see the file-wide
// `@tauri-apps/plugin-dialog` mock above), and it can be overridden per
// test with `vi.mocked(ask).mockResolvedValueOnce(...)` exactly like
// `invokeMock.mockImplementation(...)` is overridden for the saved-count
// test above. Both directions are pinned below: declined never reaches
// `notemd_vault_settings_set`; accepted reaches it with an explicit `[]`.
describe('SettingsDialog — Search & Index tab, empty pattern list confirm (design spec §8, review round 1 Important 3)', () => {
  it('declining the confirm leaves the save aborted — notemd_vault_settings_set is never called', async () => {
    // `ask`'s file-wide default already resolves `false` — no override
    // needed to exercise the decline path.
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    // No patterns configured — `globRows` starts empty, so clicking Save
    // goes straight to the empty-list confirm (design spec §8/Important 3).
    const section = namedSection('Raw source patterns')
    buttonByText(section, 'Save').click()
    await settle()

    const settingsSetCalls = invokeMock.mock.calls.filter(([cmd]) => cmd === 'notemd_vault_settings_set')
    const globsCall = settingsSetCalls.find(
      ([, args]) => args != null && typeof args === 'object' && 'searchSourceGlobs' in (args as object),
    )
    expect(globsCall, `declining must not reach notemd_vault_settings_set: ${JSON.stringify(settingsSetCalls)}`).toBeUndefined()
  })

  it('accepting the confirm proceeds to save an explicit empty list', async () => {
    vi.mocked(ask).mockResolvedValueOnce(true)

    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Raw source patterns')
    buttonByText(section, 'Save').click()
    await settle()

    const settingsSetCalls = invokeMock.mock.calls.filter(([cmd]) => cmd === 'notemd_vault_settings_set')
    const globsCall = settingsSetCalls.find(
      ([, args]) => args != null && typeof args === 'object' && 'searchSourceGlobs' in (args as object),
    )
    expect(globsCall?.[1]).toMatchObject({ searchSourceGlobs: [] })
  })
})

// Final review I-2, end to end through the UI: the settings page sends the
// whole weights draft, and `vault_settings::merge` REPLACES the stored struct
// with it — so a field the page does not carry is a field deleted from
// `settings.json`. `attention` (the attention-boost `k`) has no input here on
// purpose, which is exactly why it has to survive the round trip: hand-editing
// `settings.json` is the only way to set it, and `0` — "turn attention
// weighting off" — is the value users will set.
describe('SettingsDialog — Search & Index tab, saving tier weights preserves attention (final review I-2)', () => {
  /** Answers the settings-load command with a stored `attention`, and lets
   *  the save succeed so the payload can be inspected. */
  function routeStoredWeights(attention: number) {
    invokeMock.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'sotvault_vault_root') return '/tmp/vault'
      if (cmd === 'notemd_vault_settings_get') {
        return { searchWeights: { human: 1.25, derived: 1, source: 0.9, unlabeled: 0.3, attention } }
      }
      if (cmd === 'notemd_vault_settings_set') {
        return { searchWeights: (args as { searchWeights: unknown }).searchWeights }
      }
      throw new Error(`no tauri host in vitest: ${cmd}`)
    })
  }

  function savedWeights(): Record<string, number> {
    const call = invokeMock.mock.calls.filter((c) => c[0] === 'notemd_vault_settings_set').at(-1)
    if (!call) throw new Error('notemd_vault_settings_set was never called')
    return (call[1] as { searchWeights: Record<string, number> }).searchWeights
  }

  // Note on this first test's power, measured: with the pre-fix mirror it
  // passes anyway, because `{ ...vaultSettings.searchWeights }` carries an
  // extra runtime key the TS type never declared. What it pins is the
  // contract (the payload must carry `attention`), and the type layer +
  // `pnpm check` is what keeps the mirror honest. The `restore defaults`
  // test below is the one that goes red on the actual pre-fix code — that
  // path builds its draft from `DEFAULT_SEARCH_WEIGHTS` alone, so a missing
  // field there really does delete the stored value.
  it('a stored attention: 0 is still 0 after the user saves the four tier weights', async () => {
    routeStoredWeights(0)
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Ranking weights')
    typeInto(section.querySelector('input[type="number"]') as HTMLInputElement, '2')
    await settle()
    buttonByText(section, 'Save').click()
    await settle()

    expect(savedWeights().human).toBe(2)
    expect(savedWeights().attention, 'saving tier weights must not wipe the attention weight').toBe(0)
  })

  it('"restore defaults" resets the four tiers it is about, not the attention weight it never mentions', async () => {
    routeStoredWeights(0)
    await mountDialog()
    await settle()
    openSettings('search')
    await settle()

    const section = namedSection('Ranking weights')
    buttonByText(section, 'Restore defaults').click()
    await settle()
    buttonByText(section, 'Save').click()
    await settle()

    expect(savedWeights().human).toBe(1.25) // the tier defaults did get restored
    expect(savedWeights().attention).toBe(0)
  })
})
