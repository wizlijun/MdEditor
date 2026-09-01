// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import { setLocale } from '../lib/strings'
import MetadataSheet from './MetadataSheet.svelte'

setLocale('zh')

let component: ReturnType<typeof mount> | null = null

afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.innerHTML = ''
})

describe('MetadataSheet', () => {
  it('prefills current values and submits normalized planning metadata', async () => {
    const onSubmit = vi.fn(async () => {})
    component = mount(MetadataSheet, {
      target: document.body,
      props: {
        itemTitle: '交付版本',
        metadata: { priority: 'P1', due: '2026-09-08', contexts: ['@电脑'] },
        saving: false,
        onCancel: vi.fn(),
        onSubmit,
      },
    })
    flushSync()

    expect(document.querySelector<HTMLSelectElement>('[name="priority"]')?.value).toBe('P1')
    expect(document.querySelector<HTMLInputElement>('[name="due"]')?.value).toBe('2026-09-08')
    expect(document.querySelector<HTMLInputElement>('[name="contexts"]')?.value).toBe('@电脑')

    const priority = document.querySelector<HTMLSelectElement>('[name="priority"]')!
    priority.value = 'P0'
    priority.dispatchEvent(new Event('change', { bubbles: true }))
    const due = document.querySelector<HTMLInputElement>('[name="due"]')!
    due.value = ''
    due.dispatchEvent(new InputEvent('input', { bubbles: true }))
    const contexts = document.querySelector<HTMLInputElement>('[name="contexts"]')!
    contexts.value = '@电话， @电脑，@电话'
    contexts.dispatchEvent(new InputEvent('input', { bubbles: true }))
    document.querySelector<HTMLFormElement>('[data-form="edit-metadata"]')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))

    await vi.waitFor(() => expect(onSubmit).toHaveBeenCalledWith({
      priority: 'P0', contexts: ['@电话', '@电脑'],
    }))
  })

  it('closes with Escape only when not saving', () => {
    const onCancel = vi.fn()
    component = mount(MetadataSheet, {
      target: document.body,
      props: {
        itemTitle: '交付版本',
        metadata: { priority: 'P2', contexts: [] },
        saving: false,
        onCancel,
        onSubmit: vi.fn(),
      },
    })
    flushSync()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(onCancel).toHaveBeenCalledOnce()
  })
})
