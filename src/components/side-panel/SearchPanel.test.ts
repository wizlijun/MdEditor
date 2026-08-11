// @vitest-environment happy-dom
//
// Component test for the grouping wiring added in task B-T7: `groupHits`
// itself is covered exhaustively (with mutation checks) in
// `src/lib/search/grouping.test.ts`, a pure-function test with no component
// harness. This file exists to catch the one class of bug that kind of test
// structurally cannot — the panel rendering the WRONG thing from a correct
// `groupHits` output (mislabeled headers, groups in the wrong DOM order,
// wrong counts) — the same class of bug `SettingsDialog.search-tab.test.ts`'s
// header comment describes three prior tasks getting away with by wrongly
// believing no component-test harness existed for this app (it does:
// `ThemeImportDialog.test.ts`).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, unmount } from 'svelte'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => { throw new Error('no tauri host in vitest') }),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({
  message: vi.fn(async () => {}),
  save: vi.fn(async () => null),
  open: vi.fn(async () => null),
}))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: vi.fn(async () => undefined), set: vi.fn(async () => {}), save: vi.fn(async () => {}) })) },
}))
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))

import { searchStore, _setSearchImpl } from '../../lib/search/store.svelte'
import type { SearchHit, SearchResponse } from '../../lib/search/api'

function hit(overrides: Partial<SearchHit>): SearchHit {
  return {
    path: 'a.md', absPath: '/v/a.md', line: 1, lineEnd: 1, text: 'hit text', breadcrumb: 'a.md',
    level: 'line', score: 0.5, docDate: null, sourceRef: 'a.md#L1', agentBy: null,
    humanVerified: false, origin: 'derived', conceptType: null,
    ...overrides,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  document.body.innerHTML = ''
  searchStore.clear()
})

async function mountPanel() {
  const { default: SearchPanel } = await import('./SearchPanel.svelte')
  return mount(SearchPanel as unknown as Parameters<typeof mount>[0], {
    target: document.body,
    props: { tab: null },
  })
}

describe('SearchPanel grouping', () => {
  it('renders the two poles at the ends and a named-type group between them, each with a count', async () => {
    const hits: SearchHit[] = [
      hit({ path: 'src.md', text: 'source text', origin: 'source' }),
      hit({ path: 'ans.md', text: 'answer text', origin: 'derived', conceptType: 'Answer' }),
      hit({ path: 'human.md', text: 'human text', origin: 'human' }),
    ]
    const response: SearchResponse = { route: 't1-fts', tookMs: 1, total: hits.length, hits }
    _setSearchImpl(async () => response)

    const app = await mountPanel()
    await searchStore.run('x')
    await new Promise((r) => setTimeout(r, 0))

    const headers = Array.from(document.body.querySelectorAll('.group-label')).map((el) => el.textContent)
    // Group order must be: human pole, then the named-type group, then the
    // source pole — regardless of the order `hits` arrived in (the fixture
    // above deliberately does NOT list them pole-first).
    expect(headers).toEqual(['Written by you', 'Answer', 'Raw source material'])

    const counts = Array.from(document.body.querySelectorAll('.group-count')).map((el) => el.textContent)
    expect(counts).toEqual(['1', '1', '1'])

    // Review round 1: headers/counts alone don't prove each hit renders
    // under ITS OWN group — `{#each group.hits …}` swapped for `{#each
    // searchStore.hits …}` (every hit rendered under every header) would
    // leave both assertions above unchanged (counts read `group.hits.length`,
    // which is still 1 per group; headers don't depend on the inner loop at
    // all). Read the `.loc` row inside each `.group` and pin which hit is
    // actually nested under which header.
    const rowsPerGroup = Array.from(document.body.querySelectorAll('.group')).map((g) =>
      Array.from(g.querySelectorAll('.hit .loc')).map((el) => el.textContent),
    )
    expect(rowsPerGroup).toEqual([['human.md:1'], ['ans.md:1'], ['src.md:1']])

    unmount(app)
  })

  it('omits a pole entirely when no hit belongs to it, rather than rendering it empty', async () => {
    const hits: SearchHit[] = [
      hit({ path: 'human.md', origin: 'human' }),
      hit({ path: 'ans.md', origin: 'derived', conceptType: 'Answer' }),
    ]
    _setSearchImpl(async () => ({ route: 't1-fts', tookMs: 1, total: hits.length, hits }))

    const app = await mountPanel()
    await searchStore.run('x')
    await new Promise((r) => setTimeout(r, 0))

    const headers = Array.from(document.body.querySelectorAll('.group-label')).map((el) => el.textContent)
    expect(headers).toEqual(['Written by you', 'Answer'])
    expect(document.body.textContent).not.toContain('Raw source material')

    const rowsPerGroup = Array.from(document.body.querySelectorAll('.group')).map((g) =>
      Array.from(g.querySelectorAll('.hit .loc')).map((el) => el.textContent),
    )
    expect(rowsPerGroup).toEqual([['human.md:1'], ['ans.md:1']])

    unmount(app)
  })
})
