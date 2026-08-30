// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import { reduceEvents } from './lib/domain'
import type { NextWorkspace, WorkspaceItem } from './lib/repository'

const mocks = vi.hoisted(() => ({
  state: { workspace: null as NextWorkspace | null, loading: false, saving: false, error: null as string | null },
  refresh: vi.fn(async () => {}),
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
  return {
    ledger: { type: 'Next', version: 1, source_dirs: ['inbox/ideas'], events: [], extra: {} },
    ledgerRaw: null,
    sourceDirs: ['inbox/ideas'],
    projection: reduceEvents([]),
    sources: [],
    items: [wip, waiting, capture],
    capture: [capture],
    wip: [wip],
    waiting: [waiting],
    dormant: [],
    closed: [],
    unsupported: [],
    scanErrors: [],
    readOnlyError: null,
  }
}

describe('Next window', () => {
  it('shows current responsibilities but keeps capture hidden until requested', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    expect(document.body.textContent).toContain('正在验证的想法')
    expect(document.body.textContent).toContain('等待设计稿')
    expect(document.body.textContent).not.toContain('默认隐藏的想法')

    const button = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('安放一个想法'))
    expect(button).toBeTruthy()
    button!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('默认隐藏的想法')
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
