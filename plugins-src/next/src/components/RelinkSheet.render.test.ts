// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import RelinkSheet from './RelinkSheet.svelte'
import { setLocale } from '../lib/strings'
import type { WorkspaceItem } from '../lib/repository'

const mounted: ReturnType<typeof mount>[] = []

afterEach(() => {
  for (const component of mounted.splice(0)) unmount(component)
  document.body.innerHTML = ''
  setLocale('en')
})

function orphan(match: WorkspaceItem['relinkMatch']): WorkspaceItem {
  return {
    key: 'idea-1',
    idea_id: 'idea-1',
    state: 'wip',
    title: 'Missing source',
    path: 'old/a-idea.md',
    created: '2026-08-29T01:00:00Z',
    proofed: false,
    orphan: true,
    relinkMatch: match,
    relinkCandidates: [{
      path: 'new/a-idea.md',
      title: 'Candidate',
      created: match === 'created' ? '2026-08-29T01:00:00Z' : '2026-08-30T01:00:00Z',
      proofed: false,
    }],
  }
}

describe('RelinkSheet', () => {
  it('distinguishes exact-time candidates from unverified manual choices and traps initial keyboard focus', async () => {
    setLocale('zh')
    mounted.push(mount(RelinkSheet, {
      target: document.body,
      props: { item: orphan('manual'), saving: false, onCancel: vi.fn(), onSubmit: vi.fn() },
    }))
    flushSync()
    await Promise.resolve()

    expect(document.body.textContent).toContain('没有找到相同创建时间')
    expect(document.body.textContent).toContain('2026-08-30T01:00:00Z')
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
    expect(dialog.getAttribute('tabindex')).toBe('-1')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
    expect(dialog.contains(document.activeElement)).toBe(true)
  })

  it('supports Escape and submits only the explicitly selected source', async () => {
    const onCancel = vi.fn()
    const onSubmit = vi.fn(async () => {})
    mounted.push(mount(RelinkSheet, {
      target: document.body,
      props: { item: orphan('created'), saving: false, onCancel, onSubmit },
    }))
    flushSync()

    const radio = document.querySelector<HTMLInputElement>('input[type="radio"]')!
    radio.click()
    document.querySelector('form')!.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ path: 'new/a-idea.md' }))

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(onCancel).toHaveBeenCalledOnce()
  })
})
