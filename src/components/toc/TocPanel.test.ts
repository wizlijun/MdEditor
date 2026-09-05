// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import { createClassComponent } from 'svelte/legacy'
import type { Tab } from '../../lib/tabs.svelte'

const { setSideVisible } = vi.hoisted(() => ({
  setSideVisible: vi.fn(async () => {}),
}))
vi.mock('../../lib/side-panel/registry.svelte', () => ({
  setSideVisible,
  sideShownViews: () => [{ id: 'table-of-contents', title: () => 'Table of Contents' }],
  sideActiveView: () => ({ id: 'table-of-contents', title: () => 'Table of Contents' }),
  setActiveView: vi.fn(async () => {}),
}))

import { reveal } from '../../lib/outline/reveal.svelte'
import { t } from '../../lib/i18n/store.svelte'
import { reportTocLocation, tocLocation } from '../../lib/toc/location.svelte'
import TocPanel from './TocPanel.svelte'

function tab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: 'article',
    filePath: '/vault/article.md',
    title: 'article.md',
    initialContent: '',
    currentContent: '# Repeat\n## Child\n# Repeat',
    mode: 'rich',
    kind: 'markdown',
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: 0,
    lastKnownHash: '',
    ...overrides,
  }
}

function render(current: Tab | null) {
  const app = mount(TocPanel as unknown as Parameters<typeof mount>[0], {
    target: document.body,
    props: { tab: current },
  })
  flushSync()
  return app
}

beforeEach(() => {
  document.body.innerHTML = ''
  setSideVisible.mockClear()
  reveal.req = null
  tocLocation.trackedTabId = null
  tocLocation.activeHeadingIndex = null
})

describe('TocPanel', () => {
  it('renders a read-only hierarchy from the current article content', () => {
    const app = render(tab())
    const rows = Array.from(document.body.querySelectorAll<HTMLButtonElement>('.toc-row'))

    expect(rows.map((row) => row.querySelector('.label')?.textContent)).toEqual([
      'Repeat', 'Child', 'Repeat',
    ])
    expect(rows.map((row) => row.dataset.level)).toEqual(['1', '2', '1'])
    expect(rows.map((row) => row.style.getPropertyValue('--toc-depth').trim())).toEqual(['0', '1', '0'])
    expect(document.body.querySelector('textarea,input,[contenteditable="true"]')).toBeNull()

    unmount(app)
  })

  it('refreshes the outline when the current in-memory tab snapshot changes', () => {
    const app = createClassComponent({
      component: TocPanel,
      target: document.body,
      props: { tab: tab({ currentContent: '# Before' }) },
    })
    expect(document.body.querySelector('.label')?.textContent).toBe('Before')

    app.$set({ tab: tab({ currentContent: '# After\n## Unsaved section' }) })
    flushSync()

    expect(Array.from(document.body.querySelectorAll('.label')).map((el) => el.textContent))
      .toEqual(['After', 'Unsaved section'])
    app.$destroy()
  })

  it('sends line, path and heading index when a heading is clicked', () => {
    const app = render(tab())
    const rows = document.body.querySelectorAll<HTMLButtonElement>('.toc-row')

    rows[2].click()
    flushSync()

    expect(reveal.req).toMatchObject({
      line: 3,
      text: 'Repeat',
      path: '/vault/article.md',
      headingIndex: 2,
    })
    expect(rows[2].getAttribute('aria-current')).toBe('location')
    unmount(app)
  })

  it('highlights and reveals the heading reported by the current editor', async () => {
    const app = render(tab())
    const rows = document.body.querySelectorAll<HTMLButtonElement>('.toc-row')
    const revealRow = vi.fn()
    Object.defineProperty(rows[1], 'scrollIntoView', { value: revealRow, configurable: true })

    reportTocLocation('article', 1)
    flushSync()
    await tick()

    expect(rows[0].classList.contains('current')).toBe(false)
    expect(rows[1].classList.contains('current')).toBe(true)
    expect(rows[1].getAttribute('aria-current')).toBe('location')
    expect(revealRow).toHaveBeenCalledWith({ block: 'nearest' })
    unmount(app)
  })

  it('tracks only while the Markdown TOC is mounted', () => {
    const app = render(tab())
    expect(tocLocation.trackedTabId).toBe('article')
    unmount(app)
    expect(tocLocation).toMatchObject({ trackedTabId: null, activeHeadingIndex: null })
  })

  it('shows distinct empty states for no article and no headings', () => {
    let app = render(null)
    expect(document.body.textContent).toContain(t('toc.noDocument'))
    unmount(app)

    document.body.innerHTML = ''
    app = render(tab({ currentContent: 'Body only.' }))
    expect(document.body.textContent).toContain(t('toc.empty'))
    unmount(app)
  })

  it('shows a non-applicable state for non-article tabs', () => {
    const app = render(tab({ kind: 'canvas' }))
    expect(document.body.textContent).toContain(t('toc.notApplicable'))
    expect(document.body.querySelectorAll('.toc-row')).toHaveLength(0)
    unmount(app)
  })

  it('uses the shared right-panel hide action', () => {
    const app = render(tab())
    document.body.querySelector<HTMLButtonElement>('.hide-button')?.click()
    expect(setSideVisible).toHaveBeenCalledWith('right', false)
    unmount(app)
  })
})
