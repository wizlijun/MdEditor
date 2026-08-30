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
