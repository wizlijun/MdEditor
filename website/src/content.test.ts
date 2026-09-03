import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const publicDir = join(__dirname, '..', 'public')
const blogPath = 'blog/personal-ai-memory/index.html'
const locales = [
  { lang: 'en', prefix: '', home: 'index.html' },
  { lang: 'de', prefix: '/de', home: 'de/index.html' },
  { lang: 'ja', prefix: '/ja', home: 'ja/index.html' },
  { lang: 'zh', prefix: '/zh', home: 'zh/index.html' },
]

describe('Memory product thesis content', () => {
  it.each(locales)('links the $lang homepage to its localized Memory essay', ({ prefix, home }) => {
    const html = readFileSync(join(publicDir, home), 'utf8')
    expect(html).toContain(`href="${prefix}/blog/personal-ai-memory/"`)
    expect(html).toMatch(/<span class="n">05<\/span>/)
  })

  it.each(locales)('publishes a complete $lang essay with language alternates', ({ lang, prefix }) => {
    const html = readFileSync(join(publicDir, prefix.slice(1), blogPath), 'utf8')
    expect(html).toContain(`<html lang="${lang}">`)
    expect(html).toContain('"@type": "BlogPosting"')
    expect(html).toContain('<main><article class="wrap">')
    expect(html.match(/<h2>/g)?.length).toBeGreaterThanOrEqual(10)
    expect(html.match(/<table>/g)?.length).toBeGreaterThanOrEqual(3)
    expect(html).toContain(`rel="canonical" href="https://notemd.net${prefix}/blog/personal-ai-memory/"`)
    for (const locale of locales) {
      expect(html).toContain(
        `hreflang="${locale.lang}" href="https://notemd.net${locale.prefix}/blog/personal-ai-memory/"`,
      )
    }
    expect(html).toContain('83')
    expect(html).toContain('27')
    expect(html).toContain('56')
  })

  it('lists every localized essay URL in the sitemap', () => {
    const sitemap = readFileSync(join(publicDir, 'sitemap.xml'), 'utf8')
    const urls = Array.from(sitemap.matchAll(/<loc>([^<]+)<\/loc>/g), (match) => match[1])
    expect(urls).toHaveLength(52)
    expect(new Set(urls).size).toBe(52)
    for (const url of urls) {
      const path = new URL(url).pathname
      const file = path === '/' ? 'index.html' : `${path.slice(1)}index.html`
      expect(existsSync(join(publicDir, file)), `${url} should have a generated page`).toBe(true)
    }
    for (const locale of locales) {
      expect(sitemap).toContain(`<loc>https://notemd.net${locale.prefix}/blog/personal-ai-memory/</loc>`)
    }
  })

  it('does not describe the shipped Memory system as roadmap work', () => {
    const readme = readFileSync(join(__dirname, '..', '..', 'README.md'), 'utf8')
    const readmeZh = readFileSync(join(__dirname, '..', '..', 'README.zh-CN.md'), 'utf8')
    expect(readme).not.toContain('A memory system is on the roadmap')
    expect(readmeZh).not.toContain('记忆系统在路上')
  })
})
