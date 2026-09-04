// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import CanvasMarkdownPreview from './CanvasMarkdownPreview.svelte'

const h = vi.hoisted(() => ({ openUrl: vi.fn(async () => {}) }))

vi.mock('../../lib/plugins/host-render-html', () => ({
  renderMarkdownInline: (markdown: string) => markdown,
}))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: h.openUrl }))

describe('CanvasMarkdownPreview', () => {
  let component: ReturnType<typeof mount> | null = null

  afterEach(async () => {
    if (component) await unmount(component)
    component = null
    h.openUrl.mockClear()
    document.body.innerHTML = ''
  })

  it('removes active content and resolves only host-approved local image paths', async () => {
    component = mount(CanvasMarkdownPreview, {
      target: document.body,
      props: {
        markdown: [
          '<script>window.pwned = true</script>',
          '<img src="./ok.png" onerror="window.pwned = true" style="position:fixed">',
          '<img src="https://example.com/tracker.png">',
          '<a href="javascript:alert(1)" onclick="alert(2)">bad</a>',
        ].join(''),
        resolveLocalResource: async (src: string) => src === './ok.png' ? 'blob:safe-image' : null,
      },
    })
    await vi.waitFor(() => expect(document.querySelectorAll('img')).toHaveLength(2))

    expect(document.querySelector('script')).toBeNull()
    expect(document.querySelector('[onerror], [onclick], [style]')).toBeNull()
    const images = Array.from(document.querySelectorAll('img'))
    expect(images[0].getAttribute('src')).toBe('blob:safe-image')
    expect(images[1].hasAttribute('src')).toBe(false)
    expect(document.querySelector('a')?.hasAttribute('href')).toBe(false)
  })

  it('opens only sanitized http/https links through the system opener', async () => {
    component = mount(CanvasMarkdownPreview, {
      target: document.body,
      props: {
        markdown: '<a href="https://example.com/path?q=1">safe</a>',
      },
    })
    await vi.waitFor(() => expect(document.querySelector('a')).toBeTruthy())

    ;(document.querySelector('a') as HTMLAnchorElement).click()

    await vi.waitFor(() => expect(h.openUrl).toHaveBeenCalledWith('https://example.com/path?q=1'))
  })
})
