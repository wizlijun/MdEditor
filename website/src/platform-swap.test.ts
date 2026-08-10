// @vitest-environment happy-dom
//
// The homepage ships in its macOS form and rewrites the download CTA in place
// for Windows visitors. That swap is a few lines of inline JS with no module
// boundary, so exercise it the only way that proves anything: load the real
// public/index.html, run its script under a faked navigator, read the DOM.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeEach, describe, expect, it } from 'vitest'

const PAGES = ['index.html', 'zh/index.html', 'de/index.html', 'ja/index.html']
const publicDir = join(__dirname, '..', 'public')

function load(page: string, platform: string | null) {
  const html = readFileSync(join(publicDir, page), 'utf8')
    .replace(/^[\s\S]*?<html[^>]*>/i, '')
    .replace(/<\/html>[\s\S]*$/i, '')
    // Keep the run offline: happy-dom would otherwise try to fetch the webfonts.
    .replace(/<link\b[^>]*>/gi, '')
  document.documentElement.innerHTML = html

  if (platform) {
    Object.defineProperty(window.navigator, 'userAgentData', { value: { platform }, configurable: true })
  } else {
    Object.defineProperty(window.navigator, 'userAgentData', { value: undefined, configurable: true })
  }

  // happy-dom does not execute scripts injected via innerHTML.
  for (const el of Array.from(document.querySelectorAll('script'))) {
    if (el.textContent?.includes('data-dl')) new Function(el.textContent)()
  }
}

beforeEach(() => {
  document.documentElement.innerHTML = ''
})

describe.each(PAGES)('%s', (page) => {
  it('ships macOS-first: no JS, no swap', () => {
    load(page, null)
    const ctas = Array.from(document.querySelectorAll('[data-dl]'))
    expect(ctas.length).toBe(2)
    for (const cta of ctas) expect(cta.getAttribute('href')).toBe('/download')
    for (const span of Array.from(document.querySelectorAll('[data-plat="mac"]'))) {
      expect((span as HTMLElement).hidden).toBe(false)
    }
    for (const span of Array.from(document.querySelectorAll('[data-plat="win"]'))) {
      expect((span as HTMLElement).hidden).toBe(true)
    }
  })

  it('leaves macOS visitors alone', () => {
    load(page, 'macOS')
    for (const cta of Array.from(document.querySelectorAll('[data-dl]'))) {
      expect(cta.getAttribute('href')).toBe('/download')
    }
  })

  it('rewrites both CTAs for Windows visitors', () => {
    load(page, 'Windows')
    const ctas = Array.from(document.querySelectorAll('[data-dl]'))
    expect(ctas.length).toBe(2)
    for (const cta of ctas) {
      expect(cta.getAttribute('href')).toBe('/download?os=windows')
      // Label swapped to the localized Windows string carried in the markup…
      expect(cta.querySelector('.bl')?.textContent).toBe(cta.getAttribute('data-dl-win'))
      expect(cta.querySelector('.bl')?.textContent).toBeTruthy()
      // …and the Apple glyph replaced, not duplicated.
      expect(cta.querySelectorAll('svg').length).toBe(1)
    }
    for (const span of Array.from(document.querySelectorAll('[data-plat="mac"]'))) {
      expect((span as HTMLElement).hidden).toBe(true)
    }
    const win = Array.from(document.querySelectorAll('[data-plat="win"]')) as HTMLElement[]
    expect(win.length).toBe(2)
    for (const span of win) expect(span.hidden).toBe(false)
  })
})
