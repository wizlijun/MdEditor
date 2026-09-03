import { existsSync, readFileSync, readdirSync } from 'node:fs'
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

const readTextTree = (dir: string): string =>
  readdirSync(dir, { withFileTypes: true })
    .map((entry) => {
      const path = join(dir, entry.name)
      return entry.isDirectory() ? readTextTree(path) : readFileSync(path, 'utf8')
    })
    .join('\n')

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

describe('Agent-ready and token pricing copy', () => {
  const homepageClaims = [
    {
      home: 'index.html',
      headline: 'Agent-ready by design.<br>Use the AI you already have.',
      billing: 'note.md sells no tokens, adds no token markup, and charges no separate per-token fee.',
    },
    {
      home: 'de/index.html',
      headline: 'Von Grund auf Agent-ready.<br>Mit der KI, die du schon nutzt.',
      billing: 'note.md verkauft keine Tokens, schlägt nichts auf Tokenpreise auf und berechnet keine separate Tokengebühr.',
    },
    {
      home: 'ja/index.html',
      headline: 'AIエージェントを前提に設計。<br>いつものAIを、そのまま。',
      billing: 'note.md 独自のトークン販売、価格上乗せ、従量課金はありません。',
    },
    {
      home: 'zh/index.html',
      headline: '为 AI Agent 原生设计。<br>用你已经在用的 AI。',
      billing: 'note.md 不另售 Token，不对 Token 加价，也不另收一份按 Token 计费的使用费。',
    },
  ]

  it.each(homepageClaims)('states the Agent-ready promise accurately on $home', ({ home, headline, billing }) => {
    const html = readFileSync(join(publicDir, home), 'utf8')
    expect(html).toContain(headline)
    expect(html).toContain(billing)
  })

  it('explains the token boundary to agents without claiming zero model usage', () => {
    const llms = readFileSync(join(publicDir, 'llms.txt'), 'utf8')
    const full = readFileSync(join(publicDir, 'llms-full.txt'), 'utf8')
    for (const content of [llms, full]) {
      const normalized = content.replace(/\s+/g, ' ')
      expect(normalized).toContain('note.md includes agent-ready workflows, but it is not an AI model or token provider.')
      expect(normalized).toContain('Model usage may still consume the user\'s provider allowance')
    }
  })

  it('removes obsolete No-AI and zero-token claims from public copy and its generators', () => {
    const repositoryCopy = [
      'README.md',
      'README.zh-CN.md',
      'CLAUDE.md',
      'docs/FEATURES.md',
      'docs/FEATURES.zh-CN.md',
      'website/build_i18n.py',
      'website/build_pages.py',
      'website/i18n/pages_de.py',
      'website/i18n/pages_ja.py',
      'website/i18n/pages_zh.py',
    ]
      .map((path) => readFileSync(join(__dirname, '..', '..', path), 'utf8'))
      .join('\n')
    const copy = `${repositoryCopy}\n${readTextTree(publicDir)}`
    const obsoleteClaims = [
      'No AI inside',
      'ships no AI features of its own',
      'calls no model',
      'no API tokens, no rate limits',
      '不带 AI',
      '不连模型',
      '不会调用任何模型',
      '不要 API token，不限速',
      'Keine KI eingebaut',
      'Ruft kein Modell auf',
      'ruft selbst kein Modell auf',
      'keine API-Tokens, keine Rate-Limits',
      'AI は入っていない',
      'モデルを呼ばない',
      'モデルを呼び出したり',
      'API トークンなし、レートリミットなし',
    ]
    for (const claim of obsoleteClaims) expect(copy).not.toContain(claim)
  })
})
