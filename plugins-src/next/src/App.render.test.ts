// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import { reduceEvents } from './lib/domain'
import type { NextWorkspace, WorkspaceItem } from './lib/repository'

const mocks = vi.hoisted(() => ({
  state: { workspace: null as NextWorkspace | null, loading: false, saving: false, error: null as string | null },
  refresh: vi.fn(async () => {}),
  createIdea: vi.fn(async () => 'inbox/ideas/new-idea.md'),
  place: vi.fn(async () => {}),
  reopen: vi.fn(async () => {}),
  relink: vi.fn(async () => {}),
  open: vi.fn(async () => {}),
  toast: vi.fn(async () => {}),
}))

vi.mock('./lib/bridge', () => ({
  bridge: () => ({ locale: 'zh' }),
  toast: mocks.toast,
  vaultExists: vi.fn(),
  vaultList: vi.fn(),
  vaultRead: vi.fn(),
  vaultWrite: vi.fn(),
  editorOpen: vi.fn(),
}))

vi.mock('./lib/store.svelte', () => ({
  state: mocks.state,
  refresh: mocks.refresh,
  createIdea: mocks.createIdea,
  place: mocks.place,
  reopen: mocks.reopen,
  relink: mocks.relink,
  open: mocks.open,
}))

import App from './App.svelte'

let component: ReturnType<typeof mount> | null = null

afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
  mocks.createIdea.mockResolvedValue('inbox/ideas/new-idea.md')
  mocks.state.saving = false
})

const item = (key: string, title: string, state: WorkspaceItem['state']): WorkspaceItem => ({
  key,
  state,
  title,
  path: `inbox/ideas/${key}-idea.md`,
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
})

function workspace(): NextWorkspace {
  const wip = item('wip', '正在验证的想法', 'wip')
  const waiting = item('waiting', '等待设计稿', 'waiting')
  const capture = item('capture', '默认隐藏的想法', 'capture')
  const dormant: WorkspaceItem = {
    ...item('dormant', '以后再看的想法', 'dormant'),
    idea_id: 'dormant',
    projection: {
      idea_id: 'dormant',
      state: 'dormant',
      last_event_id: 'e-dormant',
      last_at: '2026-08-29T00:00:00Z',
      wake_trigger: '2026-12-01',
    },
  }
  const closed: WorkspaceItem = {
    ...item('closed', '已经关闭的想法', 'closed'),
    idea_id: 'closed',
    projection: {
      idea_id: 'closed',
      state: 'closed',
      last_event_id: 'e-closed',
      last_at: '2026-08-28T00:00:00Z',
      exit: { kind: 'done' },
    },
  }
  return {
    ledger: { type: 'Next', version: 1, source_dirs: ['inbox/ideas'], events: [], extra: {} },
    ledgerRaw: null,
    sourceDirs: ['inbox/ideas'],
    ideaDir: 'inbox/ideas',
    projection: reduceEvents([]),
    sources: [],
    items: [wip, waiting, capture, dormant, closed],
    capture: [capture],
    wip: [wip],
    waiting: [waiting],
    dormant: [dormant],
    closed: [closed],
    unsupported: [],
    scanErrors: [],
    readOnlyError: null,
  }
}

describe('Next window', () => {
  it('shows a New Idea shortcut and creates into the displayed Idea directory', async () => {
    const next = workspace()
    next.ideaDir = 'capture/sparks'
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const createButton = document.querySelector<HTMLButtonElement>('[data-action="new-idea"]')
    expect(createButton?.textContent).toContain('新建 Idea')
    createButton!.click()
    flushSync()
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('capture/sparks')

    const textarea = document.querySelector<HTMLTextAreaElement>('textarea[name="idea"]')!
    textarea.value = '值得记录的念头'
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLFormElement>('[data-form="create-idea"]')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await vi.waitFor(() => expect(mocks.createIdea).toHaveBeenCalledWith('值得记录的念头'))
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())
  })

  it('opens New Idea with Command-N and keeps the draft when saving fails', async () => {
    mocks.state.workspace = workspace()
    mocks.createIdea.mockRejectedValueOnce(new Error('disk full'))
    component = mount(App, { target: document.body })
    flushSync()

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', metaKey: true, bubbles: true, cancelable: true }))
    flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea[name="idea"]')!
    textarea.value = '不要丢掉我'
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLFormElement>('[data-form="create-idea"]')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await vi.waitFor(() => expect(mocks.toast).toHaveBeenCalled())
    flushSync()

    expect(document.querySelector<HTMLTextAreaElement>('textarea[name="idea"]')?.value).toBe('不要丢掉我')
  })

  it('renders five equal-width swimlanes while keeping dormant and closed memories folded', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    expect(document.querySelectorAll('[data-lane]')).toHaveLength(5)
    expect([...document.querySelectorAll('[data-lane]')].map((lane) => lane.getAttribute('data-lane'))).toEqual([
      'capture', 'wip', 'waiting', 'dormant', 'closed',
    ])
    expect(document.body.textContent).toContain('正在验证的想法')
    expect(document.body.textContent).toContain('等待设计稿')
    expect(document.body.textContent).toContain('默认隐藏的想法')
    expect(document.body.textContent).not.toContain('以后再看的想法')
    expect(document.body.textContent).not.toContain('已经关闭的想法')

    const button = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('显示已安放'))
    expect(button).toBeTruthy()
    button!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('以后再看的想法')
    expect(document.body.textContent).toContain('已经关闭的想法')
  })

  it('opens the target placement route on drop and reopens a dormant card dropped into capture', async () => {
    const next = workspace()
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const captureCard = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    captureCard.dispatchEvent(new Event('dragstart', { bubbles: true }))
    document.querySelector<HTMLElement>('[data-lane="waiting"]')!
      .dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }))
    flushSync()

    const activeRoute = document.querySelector<HTMLButtonElement>('.routes button[aria-pressed="true"]')
    expect(activeRoute?.textContent).toContain('等待回收')

    document.querySelector<HTMLButtonElement>('[aria-label="取消"]')!
      .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    const showPlaced = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('显示已安放'))!
    showPlaced.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()

    document.querySelector<HTMLElement>('[data-item-key="dormant"]')!
      .dispatchEvent(new Event('dragstart', { bubbles: true }))
    document.querySelector<HTMLElement>('[data-lane="capture"]')!
      .dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(mocks.reopen).toHaveBeenCalledWith(next.dormant[0])
  })

  it('does nothing on a same-lane drop and disables dragging while saving', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const wip = document.querySelector<HTMLElement>('[data-item-key="wip"]')!
    expect(wip.getAttribute('draggable')).toBe('true')
    wip.dispatchEvent(new Event('dragstart', { bubbles: true }))
    document.querySelector<HTMLElement>('[data-lane="wip"]')!
      .dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }))
    flushSync()
    expect(document.querySelector('[role="dialog"]')).toBeNull()

    unmount(component!)
    component = null
    document.body.innerHTML = ''
    mocks.state.saving = true
    component = mount(App, { target: document.body })
    flushSync()
    expect(document.querySelector<HTMLElement>('[data-item-key="wip"]')!.getAttribute('draggable')).toBe('false')
  })

  it('reopens a closed idea before placing it into a new lane', async () => {
    const next = workspace()
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const showPlaced = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('显示已安放'))!
    showPlaced.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    document.querySelector<HTMLElement>('[data-item-key="closed"]')!
      .dispatchEvent(new Event('dragstart', { bubbles: true }))
    document.querySelector<HTMLElement>('[data-lane="wip"]')!
      .dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }))
    flushSync()

    for (const field of ['commitment', 'nextAction', 'closeCondition']) {
      document.querySelector<HTMLButtonElement>(`[data-choices-for="${field}"] button`)!
        .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    }
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    await Promise.resolve()

    expect(mocks.reopen).toHaveBeenCalledWith(next.closed[0])
    expect(mocks.place).toHaveBeenCalledWith(next.closed[0], expect.objectContaining({ route: 'commit' }))
    expect(mocks.reopen.mock.invocationCallOrder[0]).toBeLessThan(mocks.place.mock.invocationCallOrder[0])
  })

  it('keeps a reopened orphan visible in a repair area with placement and relink actions', () => {
    const next = workspace()
    const orphan: WorkspaceItem = {
      ...item('orphan', '找不到原文的想法', 'capture'),
      idea_id: 'orphan-id',
      orphan: true,
      relinkMatch: 'manual',
    }
    next.items.push(orphan)
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    expect(document.body.textContent).toContain('需要处理')
    expect(document.body.textContent).toContain('找不到原文的想法')
    const place = [...document.querySelectorAll('button')].find((node) => node.textContent?.trim() === '安放')
    expect(place).toBeTruthy()
    place!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('安放“找不到原文的想法”')
  })
})
