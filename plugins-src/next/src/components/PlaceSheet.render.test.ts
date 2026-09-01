// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import PlaceSheet from './PlaceSheet.svelte'
import { setLocale } from '../lib/strings'
import type { WorkspaceItem } from '../lib/repository'

const mounted: ReturnType<typeof mount>[] = []

afterEach(() => {
  for (const component of mounted.splice(0)) unmount(component)
  document.body.innerHTML = ''
  setLocale('en')
})

const item: WorkspaceItem = {
  key: 'inbox/ideas/a-idea.md',
  state: 'capture',
  title: '验证一个念头',
  path: 'inbox/ideas/a-idea.md',
  created: '2026-08-29T00:00:00Z',
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
}

function input(element: HTMLTextAreaElement | HTMLInputElement, value: string) {
  element.value = value
  element.dispatchEvent(new InputEvent('input', { bubbles: true }))
}

function clickButton(label: string) {
  const button = [...document.querySelectorAll('button')].find((node) => node.textContent?.includes(label))
  expect(button).toBeTruthy()
  button!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

function change(select: HTMLSelectElement, value: string) {
  select.value = value
  select.dispatchEvent(new Event('change', { bubbles: true }))
  flushSync()
}

describe('PlaceSheet', () => {
  it('preselects a drop target route and offers four or five quick answers for each required field', () => {
    setLocale('zh')
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, initialRoute: 'wait', onCancel: vi.fn(), onSubmit: vi.fn() },
    }))
    flushSync()

    expect(document.querySelector<HTMLButtonElement>('.routes button[aria-pressed="true"]')?.textContent).toContain('等待回收')
    expect(document.querySelectorAll('[data-choices-for="waitingFor"] button')).toHaveLength(4)
    expect(document.querySelectorAll('[data-choices-for="reviewAt"] button')).toHaveLength(5)
  })

  it('fills a valid commitment from quick answers without requiring typing', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, onCancel: vi.fn(), onSubmit },
    }))
    flushSync()

    for (const field of ['commitment', 'nextAction', 'closeCondition']) {
      const choices = document.querySelectorAll<HTMLButtonElement>(`[data-choices-for="${field}"] button`)
      expect(choices).toHaveLength(4)
      choices[0].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    }
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      route: 'commit',
      commitment: expect.stringContaining('验证一个念头'),
      next_action: expect.any(String),
      close_condition: expect.any(String),
    }))
  })

  it('prefills a Task commitment from its actionable title and done condition', async () => {
    const onSubmit = vi.fn(async () => {})
    const task: WorkspaceItem = {
      ...item,
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      key: 'task-1',
      title: '提交 TestFlight 构建',
      path: 'inbox/tasks/submit-task.md',
      task: {
        version: 1,
        id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
        done_when: '构建可安装',
      },
    }
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item: task, saving: false, onCancel: vi.fn(), onSubmit },
    }))
    flushSync()

    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      route: 'commit',
      commitment: '提交 TestFlight 构建',
      next_action: '提交 TestFlight 构建',
      close_condition: '构建可安装',
    }))
  })

  it('offers choices for dormant and settlement details while preserving editable inputs', () => {
    setLocale('zh')
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, initialRoute: 'park', onCancel: vi.fn(), onSubmit: vi.fn() },
    }))
    flushSync()

    expect(document.querySelectorAll('[data-choices-for="wakeTrigger"] button')).toHaveLength(5)
    expect(document.querySelectorAll('[data-choices-for="nextAction"] button')).toHaveLength(4)
    expect(document.querySelector<HTMLInputElement>('[data-choices-for="wakeTrigger"] + input')).toBeTruthy()

    clickButton('结束或已有去处')
    change(document.querySelectorAll('select')[0], 'stopped')
    change(document.querySelectorAll('select')[1], 'drop')
    expect(document.querySelectorAll('[data-choices-for="reason"] button')).toHaveLength(4)

    change(document.querySelectorAll('select')[0], 'transferred')
    expect(document.querySelectorAll('[data-choices-for="target"] button')).toHaveLength(0)
    expect(document.querySelector<HTMLInputElement>('input[placeholder*="项目"]')).toBeTruthy()

    change(document.querySelectorAll('select')[0], 'done')
    expect(document.querySelectorAll('[data-choices-for="result"] button')).toHaveLength(4)
  })

  it('allows selecting multiple existing project tags and creating one with Enter', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: {
        item,
        saving: false,
        projectOptions: ['Next', '写作计划'],
        onCancel: vi.fn(),
        onSubmit,
      },
    }))
    flushSync()

    document.querySelector<HTMLButtonElement>('[data-project-option="Next"]')!.click()
    document.querySelector<HTMLButtonElement>('[data-project-option="写作计划"]')!.click()
    const project = document.querySelector<HTMLInputElement>('[data-project-input]')!
    input(project, ' next ')
    project.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    flushSync()
    expect(document.querySelectorAll('[data-project-tag]')).toHaveLength(2)
    input(project, '新项目')
    project.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    flushSync()
    expect([...document.querySelectorAll<HTMLElement>('[data-project-tag]')].map((tag) => tag.dataset.projectTag))
      .toEqual(['Next', '写作计划', '新项目'])
    for (const field of ['commitment', 'nextAction', 'closeCondition']) {
      document.querySelector<HTMLButtonElement>(`[data-choices-for="${field}"] button`)!
        .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    }
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      route: 'commit',
      projects: ['Next', '写作计划', '新项目'],
    }))
  })

  it('uses the project picker as the legacy target when upgrading an idea to a project', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, projectOptions: ['Next'], onCancel: vi.fn(), onSubmit },
    }))

    clickButton('结束或已有去处')
    change(document.querySelectorAll('select')[0], 'transferred')
    change(document.querySelectorAll('select')[1], 'project')
    expect(document.querySelector('input[placeholder*="人员"]')).toBeNull()

    const form = document.querySelector('form')!
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(onSubmit).not.toHaveBeenCalled()

    document.querySelector<HTMLButtonElement>('[data-project-option="Next"]')!.click()
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith({
      route: 'settle',
      exit: { kind: 'transferred', via: 'project' },
      projects: ['Next'],
      target: 'Next',
    })
  })

  it('requires an explicit target when upgrading a multi-project idea', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, projectOptions: ['Next', '写作'], onCancel: vi.fn(), onSubmit },
    }))
    clickButton('结束或已有去处')
    change(document.querySelectorAll('select')[0], 'transferred')
    change(document.querySelectorAll('select')[1], 'project')
    document.querySelector<HTMLButtonElement>('[data-project-option="Next"]')!.click()
    document.querySelector<HTMLButtonElement>('[data-project-option="写作"]')!.click()

    const form = document.querySelector('form')!
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(onSubmit).not.toHaveBeenCalled()

    change(document.querySelector<HTMLSelectElement>('[data-project-target]')!, '写作')
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith({
      route: 'settle',
      exit: { kind: 'transferred', via: 'project' },
      projects: ['Next', '写作'],
      target: '写作',
    })
  })

  it('explicitly clears an inherited project marker when the field is emptied', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    const projected: WorkspaceItem = {
      ...item,
      idea_id: 'idea-1',
      state: 'wip',
      projection: {
        idea_id: 'idea-1',
        state: 'wip',
        last_event_id: 'e1',
        last_at: '2026-08-29T00:00:00Z',
        projects: ['Next'],
        project: 'Next',
        commitment: '验证',
        next_action: '测试',
        close_condition: '结论',
      },
    }
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item: projected, saving: false, projectOptions: ['Next'], onCancel: vi.fn(), onSubmit },
    }))
    document.querySelector<HTMLButtonElement>('[data-project-tag="Next"]')!.click()
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ route: 'commit', projects: null }))
  })

  it('treats a published full article as done and requires its path or link', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, onCancel: vi.fn(), onSubmit },
    }))

    clickButton('结束或已有去处')
    change(document.querySelectorAll('select')[0], 'done')
    change(document.querySelectorAll('select')[1], 'article')
    expect(document.body.textContent).toContain('整理并发布为完整文章')

    const form = document.querySelector('form')!
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(onSubmit).not.toHaveBeenCalled()

    input(document.querySelector<HTMLInputElement>('#articleResult')!, 'writing/next-article.md')
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith({
      route: 'settle',
      exit: { kind: 'done', delivery: 'article' },
      result: 'writing/next-article.md',
    })
  })

  it('requires the three commitment fields before submitting', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, onCancel: vi.fn(), onSubmit },
    }))
    await Promise.resolve()
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
    expect(dialog.contains(document.activeElement)).toBe(true)

    const form = document.querySelector('form')!
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(document.body.textContent).toContain('请补全必填内容')
    expect(onSubmit).not.toHaveBeenCalled()

    const fields = [...document.querySelectorAll('textarea')] as HTMLTextAreaElement[]
    input(fields[0], '验证是否值得做')
    input(fields[1], '访问三个用户')
    input(fields[2], '得到明确继续或停止结论')
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith({
      route: 'commit',
      commitment: '验证是否值得做',
      next_action: '访问三个用户',
      close_condition: '得到明确继续或停止结论',
    })
  })

  it('submits waiting and dormant placements with their required recovery cues', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, onCancel: vi.fn(), onSubmit },
    }))

    clickButton('等待回收')
    input(document.querySelector('textarea')!, '设计稿')
    input(document.querySelector('input[type="date"]')!, '2026-09-02')
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenLastCalledWith({ route: 'wait', waiting_for: '设计稿', review_at: '2026-09-02' })

    clickButton('以后再看')
    input(document.querySelector('input')!, '2026-10-01')
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenLastCalledWith({ route: 'park', wake_trigger: '2026-10-01', next_action: '' })
    expect(document.body.textContent).toContain('情境只保留供搜索')
  })

  it('requires an explicit outcome and never submits fields hidden by a later outcome choice', async () => {
    setLocale('zh')
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(PlaceSheet, {
      target: document.body,
      props: { item, saving: false, onCancel: vi.fn(), onSubmit },
    }))

    clickButton('结束或已有去处')
    const form = document.querySelector('form')!
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(onSubmit).not.toHaveBeenCalled()
    expect(document.body.textContent).toContain('请补全必填内容')

    const outcome = document.querySelectorAll('select')[0]
    change(outcome, 'transferred')
    input(document.querySelector('input')!, '重要项目')
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(onSubmit).not.toHaveBeenCalled()
    change(outcome, 'stopped')
    change(document.querySelectorAll('select')[1], 'drop')
    input(document.querySelector('textarea')!, '核心假设不成立')
    form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith({
      route: 'settle',
      exit: { kind: 'stopped', via: 'drop' },
      reason: '核心假设不成立',
    })
  })
})
