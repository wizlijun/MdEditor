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
//
// The second describe block ("SearchPanel wiring") exists for a different
// reason. This file is the one the origin-tiering/query-responsiveness merge
// had to adjudicate most — 5 conflicting hunks, both sides having rewritten
// the panel — and yet every assertion above drives the panel only through
// `searchStore.run(...)`. A merge-review mutation check proved the gap: with
// the whole frontend suite green (87/87 in this file's project run), you
// could (a) revert `scheduleSearch` to a blanket 200ms debounce calling
// `searchStore.run(inputValue)` — deleting main's entire input-triggering
// rework from the panel — or (b) delete the gear button's
// `openSettings('search')` (the branch's only entry point to the Index &
// Search tab) together with `oncompositionstart`/`oncompositionend` (main's
// IME handling). Both mutations shipped silently. `input-trigger.test.ts`
// pins the pure decision function; nothing pinned that the panel *calls* it.
// `SettingsDialog.search-tab.test.ts` calls `openSettings('search')` itself;
// it pins the target, not that anything invokes it. So: assert on the
// panel's own inputs — a real `input` event, a real composition pair, a real
// click on the gear — and on the delays actually elapsed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

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

// Only `openFile` is replaced; everything else in `tabs.svelte` stays real.
// The panel's reveal call sits *after* `await openFile(...)`, so with the real
// one (which needs a Tauri host) the click is swallowed by the panel's error
// handling and the reveal assertions below could never run.
const openFileMock = vi.fn(async (_path: string) => {})
vi.mock('../../lib/tabs.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/tabs.svelte')>()),
  openFile: (path: string) => openFileMock(path),
}))

import { searchStore, _setSearchImpl } from '../../lib/search/store.svelte'
import { reveal } from '../../lib/outline/reveal.svelte'
import { uiState, closeSettings } from '../../lib/ui-state.svelte'
import { BOUNDARY_DELAY_MS, IDLE_DELAY_MS, DEEP_TIMEOUT_MS } from '../../lib/search/input-trigger'
import type { SearchHit, SearchOptions, SearchResponse } from '../../lib/search/api'

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
    // `deepAvailable: false` is load-bearing, not filler: the panel renders
    // the "search every line" hint INSTEAD of results when it is true (that
    // branch sits above the hits branch), so a fixture that left it unset
    // would render an empty panel and every assertion below would fail for
    // the wrong reason.
    const response: SearchResponse = {
      route: 't1-fts', tookMs: 1, total: hits.length, hits, truncated: false, deepAvailable: false,
    }
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
    // under ITS OWN group — `{#each group.files …}` swapped for a loop over
    // every hit in the store (every hit rendered under every header) would
    // leave both assertions above unchanged (counts read `group.hitCount`,
    // which is still 1 per group; headers don't depend on the inner loop at
    // all). Read the file rows inside each `.group` and pin which hit is
    // actually nested under which header.
    const rowsPerGroup = Array.from(document.body.querySelectorAll('.group')).map((g) =>
      Array.from(g.querySelectorAll('.file .file-name')).map((el) => el.textContent),
    )
    expect(rowsPerGroup).toEqual([['human.md'], ['ans.md'], ['src.md']])

    unmount(app)
  })

  it('omits a pole entirely when no hit belongs to it, rather than rendering it empty', async () => {
    const hits: SearchHit[] = [
      hit({ path: 'human.md', origin: 'human' }),
      hit({ path: 'ans.md', origin: 'derived', conceptType: 'Answer' }),
    ]
    _setSearchImpl(async () => ({
      route: 't1-fts', tookMs: 1, total: hits.length, hits, truncated: false, deepAvailable: false,
    }))

    const app = await mountPanel()
    await searchStore.run('x')
    await new Promise((r) => setTimeout(r, 0))

    const headers = Array.from(document.body.querySelectorAll('.group-label')).map((el) => el.textContent)
    expect(headers).toEqual(['Written by you', 'Answer'])
    expect(document.body.textContent).not.toContain('Raw source material')

    const rowsPerGroup = Array.from(document.body.querySelectorAll('.group')).map((g) =>
      Array.from(g.querySelectorAll('.file .file-name')).map((el) => el.textContent),
    )
    expect(rowsPerGroup).toEqual([['human.md'], ['ans.md']])

    unmount(app)
  })
})

// Per-file collapsing and the cleaned/highlighted preview line (design spec
// 2026-08-12 §2/§3). `grouping.test.ts` and `preview.test.ts` cover the pure
// functions; what only a mounted panel can catch is the wiring — collapsed by
// default, a click actually expanding, a one-hit file opening instead of
// expanding, and the query's terms reaching `<mark>`.
describe('SearchPanel per-file collapsing', () => {
  function respond(hits: SearchHit[]) {
    _setSearchImpl(async () => ({
      route: 't1-fts', tookMs: 1, total: hits.length, hits, truncated: false, deepAvailable: false,
    }))
  }

  async function render(hits: SearchHit[], query = 'x') {
    respond(hits)
    const app = await mountPanel()
    await searchStore.run(query)
    await new Promise((r) => setTimeout(r, 0))
    return app
  }

  function fileRows() {
    return Array.from(document.body.querySelectorAll<HTMLButtonElement>('.file-row'))
  }

  it('同文件多命中折叠成一行,展开后才出现每条命中', async () => {
    const app = await render([
      hit({ path: 'notes/a.md', line: 12, text: '外骨骼的能量回收路径', origin: 'human' }),
      hit({ path: 'notes/a.md', line: 48, text: '外骨骼与假肢的边界', origin: 'human' }),
    ], '外骨骼')

    expect(fileRows()).toHaveLength(1)
    expect(document.body.querySelector('.file-name')!.textContent).toBe('a.md')
    expect(document.body.querySelector('.file-count')!.textContent).toBe('2')
    expect(fileRows()[0].getAttribute('aria-expanded')).toBe('false')
    expect(document.body.querySelectorAll('.hit')).toHaveLength(0)

    fileRows()[0].click()
    flushSync()

    expect(fileRows()[0].getAttribute('aria-expanded')).toBe('true')
    const locs = Array.from(document.body.querySelectorAll('.hit .loc')).map((el) => el.textContent)
    expect(locs).toEqual(['12', '48'])
    // The collapsed preview is gone once the real rows are showing.
    expect(document.body.querySelectorAll('.preview')).toHaveLength(0)

    // And it collapses again.
    fileRows()[0].click()
    flushSync()
    expect(document.body.querySelectorAll('.hit')).toHaveLength(0)

    unmount(app)
  })

  it('单命中文件不是可展开控件,点击直接打开对应文件', async () => {
    const app = await render([hit({ path: 'solo.md', absPath: '/v/solo.md', text: '外骨骼', origin: 'human' })], '外骨骼')

    const row = fileRows()[0]
    expect(row.hasAttribute('aria-expanded')).toBe(false)
    row.click()
    await new Promise((r) => setTimeout(r, 0))
    flushSync()
    expect(openFileMock).toHaveBeenCalledWith('/v/solo.md')
    // The click opened the file rather than expanding a disclosure.
    expect(document.body.querySelectorAll('.hit')).toHaveLength(0)

    unmount(app)
  })

  it('点击命中跳到命中所在的行,而不是块首行', async () => {
    // The block starts at line 40; the match is its third line, so the reveal
    // has to ask for 42. Asking for `hit.line` would drop the reader at the
    // heading and leave them hunting for the match.
    const block = ['## 记忆检索', '这一行不含关键词', '外骨骼的能量回收路径'].join('\n')
    const app = await render(
      [hit({ path: 'a.md', absPath: '/v/a.md', line: 40, lineEnd: 42, text: block, origin: 'human' })],
      '外骨骼',
    )

    fileRows()[0].click()
    await new Promise((r) => setTimeout(r, 0))
    flushSync()

    expect(openFileMock).toHaveBeenCalledWith('/v/a.md')
    expect(reveal.req).toMatchObject({
      line: 42,
      // Cleaned, because rich mode matches against rendered text nodes.
      text: '外骨骼的能量回收路径',
      // Addressed, so a rebuilt editor can tell the request is meant for it.
      path: '/v/a.md',
    })

    unmount(app)
  })

  it('折叠预览取命中所在的那一行,剥掉标记并高亮关键词', async () => {
    const block = ['## 记忆检索', '这一行不含关键词', '**外**骨骼的[[髋关节|关节]]结构'].join('\n')
    const app = await render([hit({ path: 'a.md', text: block, origin: 'human' })], '外骨骼')

    const preview = document.body.querySelector('.preview')!
    expect(preview.textContent).toBe('外骨骼的关节结构')
    expect(preview.querySelector('mark')!.textContent).toBe('外骨骼')

    unmount(app)
  })

  it('围栏内的命中标出语言,不把整块 JSON 铺出来', async () => {
    const block = ['```json', '{', '  "entity_boost": 0.3,', '  "raw_score": 1.0', '}', '```'].join('\n')
    const app = await render([hit({ path: 'a.md', text: block, origin: 'human' })], 'entity_boost')

    expect(document.body.querySelector('.preview .lang')!.textContent).toBe('json')
    expect(document.body.querySelector('.preview')!.textContent).toContain('"entity_boost": 0.3,')
    expect(document.body.querySelector('.preview')!.textContent).not.toContain('raw_score')

    unmount(app)
  })

  it('换一个查询会收起上一次展开的文件', async () => {
    const hits = [
      hit({ path: 'a.md', line: 1, text: '外骨骼一', origin: 'human' }),
      hit({ path: 'a.md', line: 2, text: '外骨骼二', origin: 'human' }),
    ]
    const app = await render(hits, '外骨骼')

    fileRows()[0].click()
    flushSync()
    expect(document.body.querySelectorAll('.hit')).toHaveLength(2)

    await searchStore.run('别的词')
    await new Promise((r) => setTimeout(r, 0))
    flushSync()
    expect(document.body.querySelectorAll('.hit')).toHaveLength(0)

    unmount(app)
  })
})

describe('SearchPanel wiring', () => {
  /** Every query the panel actually issued, as `[query, deep, timeoutMs]`. */
  let calls: [string, boolean | undefined, number | undefined][]
  let response: SearchResponse

  function stubSearch() {
    calls = []
    response = { route: 't1-fts', tookMs: 1, total: 0, hits: [], truncated: false, deepAvailable: false }
    _setSearchImpl(async (q: string, o?: SearchOptions) => {
      calls.push([q, o?.deep, o?.timeoutMs])
      return response
    })
  }

  beforeEach(stubSearch)
  afterEach(() => {
    vi.useRealTimers()
    closeSettings()
  })

  function input(): HTMLInputElement {
    const el = document.body.querySelector<HTMLInputElement>('.search-input')
    if (!el) throw new Error('no .search-input rendered')
    return el
  }

  /**
   * What a keystroke really is at the DOM level: the value changes, then an
   * `input` event fires. Going through the element (rather than poking the
   * component's state) is the whole point — it is the only way the panel's
   * own `oninput` → `decideTrigger` path is exercised.
   */
  function type(value: string, isComposing = false) {
    const el = input()
    el.value = value
    const ev = new Event('input', { bubbles: true })
    // happy-dom's `Event` has no `isComposing`; the panel reads it as its
    // belt-and-braces guard for WebKit delivering `input` before
    // `compositionstart`, so a test that never sets it cannot exercise it.
    Object.defineProperty(ev, 'isComposing', { value: isComposing })
    el.dispatchEvent(ev)
  }

  it('opens the Index & Search settings tab from the gear button', async () => {
    const app = await mountPanel()
    expect(uiState.showSettings).toBe(false)

    const gear = document.body.querySelector<HTMLButtonElement>('.settings-btn')
    expect(gear, '齿轮按钮没渲染出来').not.toBeNull()
    gear!.click()
    flushSync()

    // Both halves matter: the dialog opens AND it lands on this tab. An
    // `openSettings()` with no argument would surface whatever tab was last
    // selected — the exact regression `SettingsDialog.search-tab.test.ts`'s
    // header describes, from the other end.
    expect(uiState.showSettings).toBe(true)
    expect(uiState.pendingSettingsTab).toBe('search')

    unmount(app)
  })

  it('waits out the mid-word idle delay before querying, and queries shallow', async () => {
    vi.useFakeTimers()
    const app = await mountPanel()

    type('ab')
    // One tick short of `IDLE_DELAY_MS`. This is the assertion that fails
    // under any blanket debounce shorter than it (200ms, say) — a bare
    // "eventually it searched" check would not.
    await vi.advanceTimersByTimeAsync(IDLE_DELAY_MS - 1)
    expect(calls, `不该在 ${IDLE_DELAY_MS}ms 之前就发查询`).toEqual([])

    await vi.advanceTimersByTimeAsync(1)
    // `deep: false` is part of the contract, not incidental: live typing must
    // stay on the fast tier. `searchStore.run(value)` with no options — what
    // the pre-rework panel did — arrives here as `undefined`.
    expect(calls).toEqual([['ab', false, undefined]])

    unmount(app)
  })

  it('fires fast after a word boundary instead of waiting out the idle delay', async () => {
    vi.useFakeTimers()
    const app = await mountPanel()

    type('ab ')
    await vi.advanceTimersByTimeAsync(BOUNDARY_DELAY_MS - 1)
    expect(calls).toEqual([])

    await vi.advanceTimersByTimeAsync(1)
    // Fires here, i.e. well before `IDLE_DELAY_MS` — the two delays being
    // *different* is what proves `decideTrigger`'s decision reaches the
    // panel's timer rather than a single hard-coded number.
    expect(calls).toEqual([['ab ', false, undefined]])
    expect(BOUNDARY_DELAY_MS).toBeLessThan(IDLE_DELAY_MS)

    unmount(app)
  })

  it('holds every query while an IME composition is open, and resumes when it commits', async () => {
    vi.useFakeTimers()
    const app = await mountPanel()

    const el = input()
    el.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    // A pinyin buffer. Searching `sousuo` searches for something nobody typed.
    type('sousuo')
    await vi.advanceTimersByTimeAsync(IDLE_DELAY_MS * 3)
    expect(calls, '合成中不得发起查询').toEqual([])

    // Chrome's real order: the committing `input` (still `isComposing`)
    // lands first, then `compositionend`. So the committed characters are in
    // the bound value by the time the panel is allowed to act on them.
    type('搜索', true)
    el.dispatchEvent(new Event('compositionend', { bubbles: true }))
    await vi.advanceTimersByTimeAsync(IDLE_DELAY_MS - 1)
    expect(calls, 'compositionend 之后仍须走同一套延迟规则').toEqual([])
    await vi.advanceTimersByTimeAsync(1)
    expect(calls).toEqual([['搜索', false, undefined]])

    unmount(app)
  })

  it('offers the deep-scan hint instead of "no matches", and runs a deep query when it is clicked', async () => {
    vi.useFakeTimers()
    const app = await mountPanel()
    // The fast tier missed and a scan would look further. Rendering "No
    // matches" here would be a lie the user cannot see through.
    response = { route: 't1-fts', tookMs: 1, total: 0, hits: [], truncated: false, deepAvailable: true }

    // Reached the way a user reaches it — by typing — so the hint is proven
    // to sit on the panel's own shallow-query path, not just on a store
    // state a test set by hand.
    type('zz')
    await vi.advanceTimersByTimeAsync(IDLE_DELAY_MS)
    flushSync()
    expect(calls).toEqual([['zz', false, undefined]])

    const hint = document.body.querySelector<HTMLButtonElement>('.deep-hint')
    expect(hint, 'deepAvailable 时必须渲染深搜提示').not.toBeNull()
    expect(hint!.textContent).toBe('No quick matches — search every line (slower)')
    expect(document.body.textContent).not.toContain('No matches')

    calls = []
    response = { route: 't1-scan', tookMs: 9, total: 0, hits: [], truncated: false, deepAvailable: false }
    hint!.click()
    await vi.advanceTimersByTimeAsync(0)
    // Deep, and under a budget — an unbounded deep scan measured 14s on a
    // real vault.
    expect(calls).toEqual([['zz', true, DEEP_TIMEOUT_MS]])

    flushSync()
    expect(document.body.querySelector('.deep-hint'), '深搜跑完后提示必须收起').toBeNull()

    // The auto-escalation timer `runShallow` armed must have been cancelled
    // by the click; firing it now would re-run the scan the user already got.
    await vi.advanceTimersByTimeAsync(5000)
    expect(calls, '点过深搜之后不该再自动补一次').toEqual([['zz', true, DEEP_TIMEOUT_MS]])

    unmount(app)
  })
})
