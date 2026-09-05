/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null), convertFileSrc: (s: string) => s }))
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))
vi.mock('@tauri-apps/plugin-fs', () => ({ exists: async () => false, readTextFile: async () => '', writeTextFile: async () => {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => null, save: async () => null }))

import {
  beginTocLocationTracking,
  tocLocation,
} from '../lib/toc/location.svelte'

const CONTENT = [
  '# First',
  'one',
  'two',
  'three',
  '# Repeat',
  'four',
  'five',
  'six',
  '# Repeat',
  'tail',
].join('\n')

const cleanups: Array<() => void | Promise<void>> = []

beforeEach(() => {
  document.body.innerHTML = ''
  tocLocation.trackedTabId = null
  tocLocation.activeHeadingIndex = null
})

afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((cleanup) => cleanup()))
})

async function mountSourceView() {
  const { mount, unmount } = await import('svelte')
  const SourceView = (await import('./SourceView.svelte')).default
  const target = document.createElement('div')
  document.body.appendChild(target)
  const component = mount(SourceView, {
    target,
    props: { value: CONTENT, oninput: () => {}, tabId: 'article' },
  })
  cleanups.push(() => unmount(component))
  await new Promise((resolve) => setTimeout(resolve, 20))
  const textarea = target.querySelector<HTMLTextAreaElement>('.src-textarea')!
  textarea.style.lineHeight = '20px'
  textarea.style.paddingTop = '0px'
  Object.defineProperty(textarea, 'clientHeight', { value: 80, configurable: true })
  Object.defineProperty(textarea, 'scrollHeight', { value: 320, configurable: true })
  return textarea
}

describe('SourceView TOC location tracking', () => {
  it('reports the existing reading position as soon as the TOC opens', async () => {
    const textarea = await mountSourceView()
    textarea.scrollTop = 80

    const stop = beginTocLocationTracking('article')
    cleanups.push(stop)
    await new Promise((resolve) => setTimeout(resolve, 30))

    expect(tocLocation.activeHeadingIndex).toBe(1)
  })

  it('tracks repeated headings by index and activates the final one at the bottom', async () => {
    const textarea = await mountSourceView()
    const stop = beginTocLocationTracking('article')
    cleanups.push(stop)

    textarea.scrollTop = 80
    textarea.dispatchEvent(new Event('scroll'))
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(tocLocation.activeHeadingIndex).toBe(1)

    textarea.scrollTop = 240
    textarea.dispatchEvent(new Event('scroll'))
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(tocLocation.activeHeadingIndex).toBe(2)
  })

  it('does not compute locations while the TOC is closed', async () => {
    const textarea = await mountSourceView()
    textarea.scrollTop = 240
    textarea.dispatchEvent(new Event('scroll'))
    await new Promise((resolve) => setTimeout(resolve, 30))

    expect(tocLocation).toMatchObject({ trackedTabId: null, activeHeadingIndex: null })
  })
})
