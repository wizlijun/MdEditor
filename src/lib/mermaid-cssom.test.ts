/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it } from 'vitest'
import { ensureMermaidCssStyleSheetCompatibility } from './mermaid-cssom'

const originalDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'CSSStyleSheet')

afterEach(() => {
  if (originalDescriptor) {
    Object.defineProperty(globalThis, 'CSSStyleSheet', originalDescriptor)
  } else {
    Reflect.deleteProperty(globalThis, 'CSSStyleSheet')
  }
})

describe('Mermaid CSSStyleSheet compatibility', () => {
  it('keeps a constructable native implementation', () => {
    const NativeStyleSheet = globalThis.CSSStyleSheet
    ensureMermaidCssStyleSheetCompatibility()
    expect(globalThis.CSSStyleSheet).toBe(NativeStyleSheet)
  })

  it('keeps Mermaid rendering when an old WebKit constructor throws', async () => {
    const OldWebKitStyleSheet = function CSSStyleSheet(): never {
      throw new TypeError('Illegal constructor')
    } as unknown as typeof CSSStyleSheet
    Object.defineProperty(globalThis, 'CSSStyleSheet', {
      configurable: true,
      writable: true,
      value: OldWebKitStyleSheet,
    })

    ensureMermaidCssStyleSheetCompatibility()
    const sheet = new CSSStyleSheet()
    sheet.insertRule('.node { color: rgb(1, 2, 3); }', 0)

    expect(sheet.cssRules).toHaveLength(1)
    expect(sheet.cssRules[0]?.cssText).toContain('.node')
    expect(document.querySelector('style')).toBeNull()

    Object.defineProperty(SVGElement.prototype, 'getBBox', {
      configurable: true,
      value() {
        return { x: 0, y: 0, width: 100, height: 20 }
      },
    })
    Object.defineProperty(SVGElement.prototype, 'getComputedTextLength', {
      configurable: true,
      value() {
        return 100
      },
    })

    const { default: mermaid } = await import('mermaid')
    mermaid.initialize({ startOnLoad: false, securityLevel: 'loose' })
    const quadrant = await mermaid.render(
      'legacy-cssom-quadrant',
      `quadrantChart
        x-axis 低 --> 高
        y-axis 低 --> 高
        活动甲: [0.3, 0.6]`,
    )
    expect(quadrant.svg).toContain('aria-roledescription="quadrantChart"')

    const flowchart = await mermaid.render(
      'legacy-cssom-flowchart',
      `flowchart LR
        a[开始] --> b[结束]`,
    )
    expect(flowchart.svg).toContain('class="edgePaths"')

    const c4 = await mermaid.render(
      'legacy-cssom-c4',
      `C4Context
        Person(user, "用户")
        System(app, "note.md")
        Rel(user, app, "编辑")`,
    )
    expect(c4.svg).toContain('note.md')
  })
})
