/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect } from 'vitest'
import { sanitizeInto } from './answer-card'

/**
 * 卡片正文来自 agent(vault 是多 agent 的公共地带 = 不受信任内容),
 * 而主窗口没有 CSP 且能调 Tauri IPC。这些断言守的是「答复不能变成执行通道」。
 */
describe('sanitizeInto', () => {
  const render = (html: string) => {
    const host = document.createElement('div')
    sanitizeInto(host, html)
    return host
  }

  it('keeps ordinary formatted markup', () => {
    const host = render('<p>hello <strong>world</strong></p><ul><li>a</li></ul>')
    expect(host.querySelector('strong')?.textContent).toBe('world')
    expect(host.querySelectorAll('li')).toHaveLength(1)
  })

  it('strips inline event handlers', () => {
    const host = render('<img src="x" onerror="steal()"><p onclick="x()">t</p>')
    expect(host.querySelector('img')?.hasAttribute('onerror')).toBe(false)
    expect(host.querySelector('p')?.hasAttribute('onclick')).toBe(false)
  })

  it('removes script and frame-like elements', () => {
    const host = render('<p>ok</p><script>bad()</script><iframe src="x"></iframe><object></object>')
    expect(host.querySelector('script')).toBeNull()
    expect(host.querySelector('iframe')).toBeNull()
    expect(host.querySelector('object')).toBeNull()
    expect(host.textContent).toContain('ok')
  })

  it('drops javascript: urls (including obfuscated spacing/case)', () => {
    const host = render('<a href="javascript:alert(1)">a</a><a href="JaVa\tScript:x()">b</a>')
    for (const a of host.querySelectorAll('a')) expect(a.hasAttribute('href')).toBe(false)
  })

  it('keeps safe hrefs', () => {
    const host = render('<a href="https://example.com">ok</a>')
    expect(host.querySelector('a')?.getAttribute('href')).toBe('https://example.com')
  })

  it('replaces previous content rather than appending', () => {
    const host = document.createElement('div')
    sanitizeInto(host, '<p>one</p>')
    sanitizeInto(host, '<p>two</p>')
    expect(host.querySelectorAll('p')).toHaveLength(1)
    expect(host.textContent).toBe('two')
  })
})
