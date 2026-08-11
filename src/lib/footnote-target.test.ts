// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest'
import { findAddressInText, findFootnoteTarget } from './footnote-target'

describe('findAddressInText', () => {
  it('捞出 vault 内的裸路径,不把全角括号里的说明算进去', () => {
    expect(
      findAddressInText('/2026-07-27-claude-记录写作行动-认知闭环综述.md（三元框架、判据）'),
    ).toEqual({ kind: 'href', value: '/2026-07-27-claude-记录写作行动-认知闭环综述.md' })
  })

  it('多级目录的路径', () => {
    expect(findAddressInText('/dailynote/2026/2026-07-21.note.md（摩擦不对称原始论证）')).toEqual({
      kind: 'href',
      value: '/dailynote/2026/2026-07-21.note.md',
    })
  })

  it('http 链接优先于路径', () => {
    expect(findAddressInText('见 https://example.com/a 与 /local.md')).toEqual({
      kind: 'href',
      value: 'https://example.com/a',
    })
  })

  it('去掉句末标点', () => {
    expect(findAddressInText('详见 https://example.com/a。')).toEqual({
      kind: 'href',
      value: 'https://example.com/a',
    })
  })

  it('纯文字的文献引用没有可打开的地址', () => {
    expect(
      findAddressInText('Watkins 2008, Psychological Bulletin（sources: watkins-2008）。加工模式。'),
    ).toBeNull()
  })

  it('不把普通带点的词误判成路径', () => {
    expect(findAddressInText('Cover & Thomas 的数据处理不等式 I(X;Z) ≤ I(X;Y)。')).toBeNull()
  })

  it('相对路径', () => {
    expect(findAddressInText('见 ./sub/note.md 那篇')).toEqual({
      kind: 'href',
      value: './sub/note.md',
    })
  })
})

describe('findFootnoteTarget', () => {
  function el(html: string): HTMLElement {
    const d = document.createElement('div')
    d.innerHTML = html
    return d
  }

  it('wikilink 优先', () => {
    const e = el('<span data-wikilink="某篇笔记">某篇笔记</span> 以及 /other.md')
    expect(findFootnoteTarget(e)).toEqual({ kind: 'wikilink', value: '某篇笔记' })
  })

  it('data-url 元素', () => {
    const e = el('<span data-url="https://example.com/x">标题</span>')
    expect(findFootnoteTarget(e)).toEqual({ kind: 'href', value: 'https://example.com/x' })
  })

  it('markdown 链接渲染出的 a[href]', () => {
    const e = el('<a href="https://example.com/y">How Wispr Flow Grows</a>')
    expect(findFootnoteTarget(e)).toEqual({ kind: 'href', value: 'https://example.com/y' })
  })

  it('没有链接元素时回退到文本里的裸路径', () => {
    const e = el('<p>/2026-07-27-note.md（说明）</p>')
    expect(findFootnoteTarget(e)).toEqual({ kind: 'href', value: '/2026-07-27-note.md' })
  })

  it('整条都是文字说明时返回 null', () => {
    const e = el('<p>Trapnell &amp; Campbell 1999。反刍与反思的动机区分。</p>')
    expect(findFootnoteTarget(e)).toBeNull()
  })
})
