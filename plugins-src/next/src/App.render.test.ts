// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import { reduceEvents } from './lib/domain'
import type { NextWorkspace, WorkspaceItem } from './lib/repository'
import type { CreateTaskResult } from './lib/store.svelte'

const mocks = vi.hoisted(() => ({
  state: { workspace: null as NextWorkspace | null, loading: false, saving: false, error: null as string | null },
  refresh: vi.fn(async () => {}),
  createIdea: vi.fn(async () => 'inbox/ideas/new-idea.md'),
  createTask: vi.fn(async (): Promise<CreateTaskResult> => ({ path: 'inbox/tasks/new-task.md', placedCurrent: false })),
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
  createTask: mocks.createTask,
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
  mocks.createTask.mockResolvedValue({ path: 'inbox/tasks/new-task.md', placedCurrent: false })
  mocks.state.saving = false
  vi.useRealTimers()
})

const item = (key: string, title: string, state: WorkspaceItem['state']): WorkspaceItem => ({
  key,
  state,
  title,
  body: `# ${title}\n\n第一段。\n\n这是完整 Idea 的最后一段。`,
  path: `inbox/ideas/${key}-idea.md`,
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
})

function workspace(): NextWorkspace {
  const wip: WorkspaceItem = {
    ...item('wip', '正在验证的想法', 'wip'),
    idea_id: 'wip',
    projection: {
      idea_id: 'wip',
      state: 'wip',
      last_event_id: 'e-wip',
      last_at: '2026-08-30T00:00:00Z',
      projects: ['Next', 'Research', 'Product'],
      project: 'Next',
      commitment: '验证项目标记',
      next_action: '运行测试',
      close_condition: '可以复用',
    },
  }
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
    projectOptions: [],
    scanErrors: [],
    readOnlyError: null,
  }
}

function targetLane(lane: string, left = 300): HTMLElement {
  const target = document.querySelector<HTMLElement>(`[data-lane="${lane}"]`)!
  target.getBoundingClientRect = () => ({
    x: left, y: 0, left, top: 0, right: left + 200, bottom: 500,
    width: 200, height: 500, toJSON: () => ({}),
  })
  return target
}

function pointerDrag(itemKey: string, lane: string, pointerId = 1): void {
  targetLane(lane)
  document.querySelector<HTMLElement>(`[data-item-key="${itemKey}"]`)!
    .dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId, clientX: 10, clientY: 10,
    }))
  window.dispatchEvent(new PointerEvent('pointermove', {
    bubbles: true, pointerId, clientX: 350, clientY: 50,
  }))
  window.dispatchEvent(new PointerEvent('pointerup', {
    bubbles: true, pointerId, clientX: 350, clientY: 50,
  }))
  flushSync()
}

describe('Next window', () => {
  it('shows multiple project tags compactly on its card', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()
    expect([...document.querySelectorAll('[data-item-key="wip"] .badge.project')].map((badge) => badge.textContent))
      .toEqual(['Next', 'Research', '+1'])
  })

  it('shows a local Inbox project suggestion and requires a click before selecting it', () => {
    const next = workspace()
    next.capture[0].suggestedProject = {
      project: 'Next',
      reason: 'content',
      score: 4,
      matchedTerms: ['念头', '泳道'],
      candidatesScored: 1,
    }
    next.projectOptions = ['Next']
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    expect(document.querySelector('[data-project-tag="Next"]')).toBeNull()
    document.querySelector<HTMLButtonElement>('[data-project-suggestion="Next"]')!.click()
    flushSync()
    expect(document.querySelector('[role="dialog"] [data-project-tag="Next"]')).toBeTruthy()
    expect(mocks.place).not.toHaveBeenCalled()
  })

  it('previews the complete Idea body in a viewport tip on hover and keyboard focus', () => {
    vi.useFakeTimers()
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const card = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    expect(document.querySelector('[role="tooltip"]')).toBeNull()

    card.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    let tip = document.querySelector<HTMLElement>('[role="tooltip"]')!
    expect(tip.textContent).toContain('# 默认隐藏的想法')
    expect(tip.textContent).toContain('这是完整 Idea 的最后一段。')
    expect(tip.closest('.board-scroll')).toBeNull()
    expect(card.getAttribute('aria-describedby')).toBe(tip.id)
    const waitingCard = document.querySelector<HTMLElement>('[data-item-key="waiting"]')!
    expect(waitingCard.getAttribute('aria-describedby')).not.toBe(card.getAttribute('aria-describedby'))
    expect(document.getElementById(card.getAttribute('aria-labelledby')!)?.textContent).toBe('默认隐藏的想法')
    expect(tip.classList.contains('idea-preview-tip')).toBe(true)

    card.dispatchEvent(new PointerEvent('pointerleave', { relatedTarget: tip }))
    tip.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBe(tip)
    tip.dispatchEvent(new PointerEvent('pointerleave'))
    vi.advanceTimersByTime(101)
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()

    card.focus()
    flushSync()
    tip = document.querySelector<HTMLElement>('[role="tooltip"]')!
    expect(tip.textContent).toContain('这是完整 Idea 的最后一段。')
    Object.defineProperty(tip, 'clientHeight', { configurable: true, value: 200 })
    Object.defineProperty(tip, 'scrollHeight', { configurable: true, value: 1000 })
    card.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageDown', bubbles: true, cancelable: true }))
    expect(tip.scrollTop).toBe(160)
    card.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }))
    expect(tip.scrollTop).toBe(1000)
    card.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }))
    expect(tip.scrollTop).toBe(0)

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
  })

  it('renders Idea Markdown as text and does not preview a missing source', () => {
    vi.useFakeTimers()
    const next = workspace()
    next.capture[0].body = '<img src=x onerror="alert(1)">\n\n最后一行'
    next.wip[0].body = undefined
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const capture = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    capture.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    const tip = document.querySelector<HTMLElement>('[role="tooltip"]')!
    expect(tip.textContent).toContain('<img src=x onerror="alert(1)">')
    expect(tip.querySelector('img')).toBeNull()

    capture.dispatchEvent(new PointerEvent('pointerleave'))
    document.querySelector<HTMLElement>('[data-item-key="wip"]')!
      .dispatchEvent(new PointerEvent('pointerenter'))
    vi.advanceTimersByTime(101)
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
  })

  it('keeps the tip reachable across the pointer gap and respects independent keyboard focus', () => {
    vi.useFakeTimers()
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const card = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    card.dispatchEvent(new PointerEvent('pointerenter'))
    card.focus()
    flushSync()
    const tip = document.querySelector<HTMLElement>('[role="tooltip"]')!

    card.dispatchEvent(new PointerEvent('pointerleave'))
    vi.advanceTimersByTime(200)
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBe(tip)

    card.blur()
    card.dispatchEvent(new PointerEvent('pointerenter'))
    card.dispatchEvent(new PointerEvent('pointerleave'))
    vi.advanceTimersByTime(60)
    tip.dispatchEvent(new PointerEvent('pointerenter'))
    vi.advanceTimersByTime(200)
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBe(tip)

    tip.dispatchEvent(new PointerEvent('pointerleave'))
    vi.advanceTimersByTime(200)
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
  })

  it('closes the tip before opening a card sheet and previews supported repair cards', () => {
    const next = workspace()
    const unsupported: WorkspaceItem = {
      ...item('unsupported', '需要修复的想法', 'unsupported'),
      idea_id: 'unsupported',
      projection: {
        idea_id: 'unsupported',
        state: 'unsupported',
        last_event_id: 'unsupported-event',
        last_at: '2026-08-30T00:00:00Z',
        unsupported_actions: ['future'],
      },
    }
    next.items.push(unsupported)
    next.unsupported.push(unsupported)
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const repairCard = document.querySelector<HTMLElement>('[data-item-key="unsupported"]')!
    repairCard.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain('需要修复的想法')

    const capture = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    capture.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeTruthy()
    capture.querySelector<HTMLButtonElement>('.place')!.click()
    flushSync()
    expect(document.querySelector('[role="dialog"]')).toBeTruthy()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
  })

  it('uses pointer movement to place a card because Tauri swallows HTML5 drag events', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const waitingLane = targetLane('waiting')
    const captureCard = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    captureCard.dispatchEvent(new PointerEvent('pointerenter'))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeTruthy()
    captureCard.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId: 7, clientX: 10, clientY: 10,
    }))
    captureCard.dispatchEvent(new FocusEvent('focusin', { bubbles: true }))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
    window.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 7, clientX: 350, clientY: 50,
    }))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
    expect(waitingLane.classList.contains('over')).toBe(true)
    expect(document.querySelector('.drag-ghost')?.textContent).toContain('默认隐藏的想法')
    window.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 7, clientX: 350, clientY: 50,
    }))
    flushSync()

    expect(document.querySelector<HTMLButtonElement>('.routes button[aria-pressed="true"]')?.textContent)
      .toContain('等待回收')
  })

  it('restores the focused preview after a click that never becomes a drag', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const card = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    card.dispatchEvent(new PointerEvent('pointerenter'))
    card.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId: 12, clientX: 10, clientY: 10,
    }))
    card.focus()
    flushSync()
    expect(document.querySelector('[role="tooltip"]')).toBeNull()

    window.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 12, clientX: 10, clientY: 10,
    }))
    flushSync()
    expect(document.querySelector('[role="tooltip"]')?.textContent).toContain('默认隐藏的想法')
  })

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

  it('creates a Task in Inbox or explicitly marks it current', async () => {
    const next = workspace()
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const createButton = document.querySelector<HTMLButtonElement>('[data-action="new-task"]')
    expect(createButton?.textContent).toContain('新建任务')
    createButton!.click()
    flushSync()
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('inbox/tasks')

    const title = document.querySelector<HTMLInputElement>('input[name="title"]')!
    title.value = '提交 TestFlight 构建'
    title.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLFormElement>('[data-form="create-task"]')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await vi.waitFor(() => expect(mocks.createTask).toHaveBeenCalledWith({
      title: '提交 TestFlight 构建',
    }, false))
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"]')).toBeNull())

    createButton!.click()
    flushSync()
    const currentTitle = document.querySelector<HTMLInputElement>('input[name="title"]')!
    currentTitle.value = '验证安装'
    currentTitle.dispatchEvent(new InputEvent('input', { bubbles: true }))
    const done = document.querySelector<HTMLInputElement>('input[name="done_when"]')!
    done.value = '安装成功'
    done.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLButtonElement>('[data-action="create-current"]')!.click()
    await vi.waitFor(() => expect(mocks.createTask).toHaveBeenCalledWith({
      title: '验证安装',
      done_when: '安装成功',
    }, true))
  })

  it('closes the Task sheet and warns when the file was saved but Inbox refresh failed', async () => {
    mocks.state.workspace = workspace()
    mocks.createTask.mockResolvedValueOnce({
      path: 'inbox/tasks/published-task.md',
      placedCurrent: false,
      refreshError: 'refresh unavailable',
    })
    component = mount(App, { target: document.body })
    flushSync()

    document.querySelector<HTMLButtonElement>('[data-action="new-task"]')!.click()
    flushSync()
    const title = document.querySelector<HTMLInputElement>('input[name="title"]')!
    title.value = '已发布任务'
    title.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLFormElement>('[data-form="create-task"]')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))

    await vi.waitFor(() => expect(mocks.toast).toHaveBeenCalledWith(
      'warn',
      '任务已保存，但 Next 暂时无法刷新收件箱。',
      'refresh unavailable',
    ))
    expect(document.querySelector('[role="dialog"]')).toBeNull()
  })

  it('states that mark-current did not happen when its first refresh failed', async () => {
    mocks.state.workspace = workspace()
    mocks.createTask.mockResolvedValueOnce({
      path: 'inbox/tasks/published-task.md',
      placedCurrent: false,
      refreshError: 'refresh unavailable',
    })
    component = mount(App, { target: document.body })
    flushSync()

    document.querySelector<HTMLButtonElement>('[data-action="new-task"]')!.click()
    flushSync()
    const title = document.querySelector<HTMLInputElement>('input[name="title"]')!
    title.value = '已发布任务'
    title.dispatchEvent(new InputEvent('input', { bubbles: true }))
    const done = document.querySelector<HTMLInputElement>('input[name="done_when"]')!
    done.value = '可验收'
    done.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLButtonElement>('[data-action="create-current"]')!.click()

    await vi.waitFor(() => expect(mocks.toast).toHaveBeenCalledWith(
      'warn',
      '任务已保存，但收件箱刷新失败，尚未标记为当前。',
      'refresh unavailable',
    ))
  })

  it('opens New Task with Shift-Command-N without changing the New Idea shortcut', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    window.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'n', metaKey: true, shiftKey: true, bubbles: true, cancelable: true,
    }))
    flushSync()
    expect(document.querySelector('[data-form="create-task"]')).toBeTruthy()
    expect(document.querySelector('[data-form="create-idea"]')).toBeNull()
  })

  it('marks Task and Agent provenance on cards without creating a new lane', () => {
    const next = workspace()
    const task: WorkspaceItem = {
      ...item('task-card', '提交构建', 'capture'),
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      path: 'inbox/tasks/submit-task.md',
      task: {
        version: 1,
        id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
        due: '2026-09-02',
      },
      generatedBy: 'daily-summary-agent/1',
    }
    next.items.push(task)
    next.capture.unshift(task)
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const card = document.querySelector('[data-item-key="task-card"]')!
    expect(card.textContent).toContain('任务')
    expect(card.textContent).toContain('Agent 添加')
    expect(card.textContent).toContain('2026-09-02')
    expect(document.querySelectorAll('[data-lane]')).toHaveLength(5)
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
    expect([...document.querySelectorAll('[data-lane] h2')].map((heading) => heading.textContent)).toEqual([
      '收件箱', '进行中', '等待', '稍后', '已完成',
    ])
    expect(document.querySelector('.project-filters')).toBeNull()
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

  it('lists projects after the subtitle and toggles a project filter that composes with search', () => {
    const next = workspace()
    next.projectOptions = ['Next', 'Writing', 'Legacy']
    next.waiting[0].projection = {
      idea_id: 'waiting',
      state: 'waiting',
      last_event_id: 'e-waiting',
      last_at: '2026-08-31T00:00:00Z',
      project: 'Writing',
      waiting_for: '设计稿',
      review_at: '2026-09-01T00:00:00Z',
    }
    next.closed[0].projection = {
      idea_id: 'closed',
      state: 'closed',
      last_event_id: 'e-closed',
      last_at: '2026-08-28T00:00:00Z',
      project: 'Next',
      exit: { kind: 'transferred', via: 'project' },
      target: 'Legacy',
    }
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const subtitle = document.querySelector('.subtitle-row > p')
    const filters = subtitle?.nextElementSibling
    expect(filters?.classList.contains('project-filters')).toBe(true)
    const buttons = [...document.querySelectorAll<HTMLButtonElement>('[data-project-filter]')]
    expect(buttons.map((button) => button.textContent)).toEqual(['Next', 'Writing', 'Legacy'])

    buttons[0].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(buttons[0].getAttribute('aria-pressed')).toBe('true')
    expect(document.body.textContent).toContain('正在验证的想法')
    expect(document.body.textContent).toContain('已经关闭的想法')
    expect(document.body.textContent).not.toContain('等待设计稿')
    expect(document.body.textContent).not.toContain('默认隐藏的想法')
    expect([...document.querySelectorAll('button')].some((button) => button.textContent?.includes('显示已安放'))).toBe(false)

    buttons[0].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(buttons[0].getAttribute('aria-pressed')).toBe('false')
    expect(document.body.textContent).toContain('等待设计稿')
    expect(document.body.textContent).toContain('默认隐藏的想法')
    expect(document.body.textContent).not.toContain('已经关闭的想法')
    expect([...document.querySelectorAll('button')].some((button) => button.textContent?.includes('显示已安放'))).toBe(true)

    buttons[2].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('已经关闭的想法')
    expect(document.body.textContent).not.toContain('正在验证的想法')

    buttons[1].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('等待设计稿')
    expect(document.body.textContent).not.toContain('正在验证的想法')

    const search = document.querySelector<HTMLInputElement>('input[type="search"]')!
    search.value = '不存在'
    search.dispatchEvent(new InputEvent('input', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).not.toContain('等待设计稿')
    search.value = '设计稿'
    search.dispatchEvent(new InputEvent('input', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('等待设计稿')
  })

  it('applies project and text filters together to repair cards', () => {
    const next = workspace()
    const repair: WorkspaceItem = {
      ...item('repair', '需要修复的 Next 想法', 'unsupported'),
      idea_id: 'repair',
      projection: {
        idea_id: 'repair',
        state: 'unsupported',
        last_event_id: 'e-repair',
        last_at: '2026-08-31T00:00:00Z',
        project: 'Next',
        unsupported_actions: ['future-action'],
      },
    }
    next.items.push(repair)
    next.unsupported.push(repair)
    next.projectOptions = ['Next']
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    document.querySelector<HTMLButtonElement>('[data-project-filter="Next"]')!
      .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('需要修复的 Next 想法')

    const search = document.querySelector<HTMLInputElement>('input[type="search"]')!
    search.value = '不存在'
    search.dispatchEvent(new InputEvent('input', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).not.toContain('需要修复的 Next 想法')
    search.value = '需要修复'
    search.dispatchEvent(new InputEvent('input', { bubbles: true }))
    flushSync()
    expect(document.body.textContent).toContain('需要修复的 Next 想法')
  })

  it('opens the target placement route on pointer drop and reopens a dormant card dropped into capture', async () => {
    const next = workspace()
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    pointerDrag('capture', 'waiting')

    const activeRoute = document.querySelector<HTMLButtonElement>('.routes button[aria-pressed="true"]')
    expect(activeRoute?.textContent).toContain('等待回收')

    document.querySelector<HTMLButtonElement>('[aria-label="取消"]')!
      .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    const showPlaced = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('显示已安放'))!
    showPlaced.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()

    pointerDrag('dormant', 'capture', 2)
    await Promise.resolve()
    expect(mocks.reopen).toHaveBeenCalledWith(next.dormant[0])
  })

  it('does nothing on a same-lane pointer drop and disables dragging while saving', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()

    const wip = document.querySelector<HTMLElement>('[data-item-key="wip"]')!
    expect(wip.dataset.draggable).toBe('true')
    pointerDrag('wip', 'wip')
    expect(document.querySelector('[role="dialog"]')).toBeNull()

    unmount(component!)
    component = null
    document.body.innerHTML = ''
    mocks.state.saving = true
    component = mount(App, { target: document.body })
    flushSync()
    expect(document.querySelector<HTMLElement>('[data-item-key="wip"]')!.dataset.draggable).toBe('false')
  })

  it('reopens a closed idea before placing it into a new lane', async () => {
    const next = workspace()
    mocks.state.workspace = next
    component = mount(App, { target: document.body })
    flushSync()

    const showPlaced = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes('显示已安放'))!
    showPlaced.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    flushSync()
    pointerDrag('closed', 'wip')

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

  it('cancels a pointer drag with Escape and never starts one from a card button', () => {
    mocks.state.workspace = workspace()
    component = mount(App, { target: document.body })
    flushSync()
    targetLane('waiting')

    const card = document.querySelector<HTMLElement>('[data-item-key="capture"]')!
    card.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId: 8, clientX: 10, clientY: 10,
    }))
    window.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 8, clientX: 350, clientY: 50,
    }))
    flushSync()
    expect(document.querySelector('.drag-ghost')).toBeTruthy()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    window.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 8, clientX: 350, clientY: 50,
    }))
    flushSync()
    expect(document.querySelector('[role="dialog"]')).toBeNull()

    card.querySelector<HTMLButtonElement>('button')!.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId: 9, clientX: 10, clientY: 10,
    }))
    window.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 9, clientX: 350, clientY: 50,
    }))
    window.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 9, clientX: 350, clientY: 50,
    }))
    flushSync()
    expect(document.querySelector('[role="dialog"]')).toBeNull()

    card.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, button: 0, pointerId: 10, clientX: 10, clientY: 10,
    }))
    window.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 10, clientX: 350, clientY: 50,
    }))
    window.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 10, clientX: 900, clientY: 50,
    }))
    flushSync()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
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
