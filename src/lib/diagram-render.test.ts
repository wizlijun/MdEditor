/**
 * @vitest-environment jsdom
 */
import { beforeAll, describe, expect, it } from 'vitest'
import { renderDiagrams } from './diagram-render'

const CHINESE_QUADRANT = `quadrantChart
  title 活动触达与参与
  x-axis 低触达 --> 高触达
  y-axis 低参与 --> 高参与
  quadrant-1 扩大投入
  quadrant-2 Café / 加强推广
  quadrant-3 重新评估
  quadrant-4 可以改善
  活动甲: [0.3, 0.6]
  活动乙📈: [0.45, 0.23]`

beforeAll(() => {
  // Mermaid 11.15+ creates scoped styles through the browser CSSOM. Some DOM
  // runners expose CSSStyleSheet on window without mirroring it globally.
  if (typeof globalThis.CSSStyleSheet === 'undefined') {
    Object.defineProperty(globalThis, 'CSSStyleSheet', {
      configurable: true,
      value: window.CSSStyleSheet,
    })
  }

  // jsdom does not implement SVG geometry. Mermaid's layout code only needs
  // deterministic dimensions here; browser layout is exercised by the
  // production build while these tests protect the generated SVG structure.
  Object.defineProperty(SVGElement.prototype, 'getBBox', {
    configurable: true,
    value() {
      const text = this.textContent ?? ''
      return { x: 0, y: 0, width: Math.max(40, text.length * 8), height: 20 }
    },
  })
  Object.defineProperty(SVGElement.prototype, 'getComputedTextLength', {
    configurable: true,
    value() {
      return Math.max(40, (this.textContent ?? '').length * 8)
    },
  })
})

async function renderThroughExportPipeline(source: string): Promise<HTMLElement> {
  const staging = document.createElement('div')
  const pre = document.createElement('pre')
  const code = document.createElement('code')
  code.className = 'language-mermaid'
  code.textContent = source
  pre.appendChild(code)
  staging.appendChild(pre)
  document.body.appendChild(staging)
  await renderDiagrams(staging)
  return staging
}

describe('Mermaid rendering', () => {
  it('parses unquoted Unicode architecture titles through mermaid-mini', async () => {
    const { default: mermaid } = await import('mermaid-mini')
    const result = await mermaid.parse(`architecture-beta
      group g1(server)[核心 / 台北]
      service a(server)[采集器-v2] in g1`)

    expect(result.diagramType).toBe('architecture')
  })

  it('renders an unquoted Chinese quadrant through the rich-editor renderer', async () => {
    const { renderMermaid } = await import('@moraya/core/plugins/mermaid-renderer')
    const result = await renderMermaid(CHINESE_QUADRANT)

    expect(result).toHaveProperty('svg')
    if (!('svg' in result)) throw new Error(result.error)
    expect(result.svg).toContain('aria-roledescription="quadrantChart"')
    expect(result.svg).toContain('活动甲')
    expect(result.svg).toContain('扩大投入')
  })

  it('renders the same quadrant through the export and share pipeline', async () => {
    const staging = await renderThroughExportPipeline(CHINESE_QUADRANT)

    try {
      const svg = staging.querySelector('svg')
      expect(staging.querySelector('.renderer-error')).toBeNull()
      expect(svg?.getAttribute('aria-roledescription')).toBe('quadrantChart')
      expect(svg?.querySelectorAll('.quadrant')).toHaveLength(4)
      expect(svg?.querySelectorAll('.data-point')).toHaveLength(2)
      expect(svg?.textContent).toContain('活动乙📈')
    } finally {
      staging.remove()
    }
  })

  it('keeps flowchart edge groups, markers, and classDef styles', async () => {
    const staging = await renderThroughExportPipeline(`flowchart LR
      start[开始] --> decision{判断}
      decision -->|是| done[完成]
      classDef accent fill:#123456,color:#ffffff,stroke:#abcdef
      class done accent`)

    try {
      const svg = staging.querySelector('svg')
      expect(staging.querySelector('.renderer-error')).toBeNull()
      expect(svg?.getAttribute('aria-roledescription')).toBe('flowchart-v2')
      expect(svg?.querySelector('g.edgePaths')).not.toBeNull()
      expect(svg?.querySelector('path[marker-end]')).not.toBeNull()
      expect(svg?.textContent).toContain('判断')
      expect(svg?.innerHTML).toContain('#123456')
    } finally {
      staging.remove()
    }
  })

  it('renders class relation markers with the unified renderer', async () => {
    const staging = await renderThroughExportPipeline(`classDiagram
      class Animal
      class Dog
      Animal <|-- Dog`)

    try {
      const svg = staging.querySelector('svg')
      expect(staging.querySelector('.renderer-error')).toBeNull()
      expect(svg?.getAttribute('aria-roledescription')).toBe('classDiagram')
      expect(svg?.querySelector('path[marker-end], path[marker-start]')).not.toBeNull()
      expect(svg?.textContent).toContain('Animal')
      expect(svg?.textContent).toContain('Dog')
    } finally {
      staging.remove()
    }
  })

  it('keeps C4 labels and relations in SVG for export', async () => {
    const staging = await renderThroughExportPipeline(`C4Context
      title 系统上下文
      Person(user, "用户", "撰写与整理笔记的人")
      System(app, "note.md", "本地优先的 Markdown 编辑器")
      Rel(user, app, "编辑")`)

    try {
      const svg = staging.querySelector('svg')
      expect(staging.querySelector('.renderer-error')).toBeNull()
      expect(svg?.textContent).toContain('系统上下文')
      expect(svg?.textContent).toContain('note.md')
      expect(svg?.textContent).toContain('本地优先的 Markdown 编辑器')
      expect(svg?.textContent).toContain('编辑')
    } finally {
      staging.remove()
    }
  })
})
