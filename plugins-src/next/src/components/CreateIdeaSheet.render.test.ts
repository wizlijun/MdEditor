// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import { setLocale } from '../lib/strings'
import CreateIdeaSheet from './CreateIdeaSheet.svelte'

setLocale('zh')

let component: ReturnType<typeof mount> | null = null

afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.innerHTML = ''
})

describe('CreateIdeaSheet', () => {
  it('requires the only unavoidable input and submits with Command-Enter', async () => {
    const onSubmit = vi.fn(async () => {})
    component = mount(CreateIdeaSheet, {
      target: document.body,
      props: { ideaDir: 'inbox/ideas', saving: false, onCancel: vi.fn(), onSubmit },
    })
    flushSync()

    document.querySelector<HTMLFormElement>('form')!
      .dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    flushSync()
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('写下')
    expect(onSubmit).not.toHaveBeenCalled()

    const textarea = document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '一个念头'
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true }))
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith('一个念头')
  })

  it('closes with Escape only when it is not saving', () => {
    const onCancel = vi.fn()
    component = mount(CreateIdeaSheet, {
      target: document.body,
      props: { ideaDir: 'inbox/ideas', saving: false, onCancel, onSubmit: vi.fn() },
    })
    flushSync()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(onCancel).toHaveBeenCalledOnce()
  })
})
