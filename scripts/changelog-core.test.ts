import { describe, it, expect } from 'vitest'
// @ts-expect-error - plain-JS core shared with the CLI (same pattern as insights-report-core)
import { checkGate, rotate, sectionFor, hasUnreleasedContent, parse } from './changelog-core.mjs'

/**
 * 这条路径的失败代价不对称:门禁误拦只是烦人,而**正文提取取错段落**会把上
 * 一个版本的说明当成本次发出去 —— 看起来完全正常,没人会发现。所以提取那组
 * 用例比门禁那组更细。
 */

const EN = `# Changelog

前言。

## Unreleased

## v6.817.1 — 2026-08-17

### Added

- 甲

## v6.813.6 — 2026-08-13

### Fixed

- 乙
`

const ZH = EN.replace('## Unreleased', '## 未发布')

describe('parse', () => {
  it('切出前言、未发布区与版本序列', () => {
    const p = parse(EN, 'en')
    expect(p.versions.map((v: any) => v.version)).toEqual(['6.817.1', '6.813.6'])
    expect(p.versions[0].date).toBe('2026-08-17')
  })

  it('缺「未发布」标题时报错而不是静默当空', () => {
    expect(() => parse('# Changelog\n\n## v1.0.0 — 2026-01-01\n', 'en')).toThrow(/Unreleased/)
  })
})

describe('hasUnreleasedContent', () => {
  it('空区不算已写', () => {
    expect(hasUnreleasedContent(EN, 'en')).toBe(false)
  })

  it('有条目算已写', () => {
    expect(hasUnreleasedContent(EN.replace('## Unreleased\n', '## Unreleased\n\n- 新东西\n'), 'en')).toBe(true)
  })

  /** 轮转会留下一个空区;若注释被当成内容,门禁就永远放行了。 */
  it('只有注释不算已写', () => {
    const withComment = EN.replace('## Unreleased\n', '## Unreleased\n\n<!-- 往这里加 -->\n')
    expect(hasUnreleasedContent(withComment, 'en')).toBe(false)
  })
})

describe('checkGate', () => {
  const filled = (t: string, h: string) => t.replace(`${h}\n`, `${h}\n\n- 新东西\n`)

  it('两份都写了且序列一致 → 放行', () => {
    expect(checkGate(filled(EN, '## Unreleased'), filled(ZH, '## 未发布'))).toEqual([])
  })

  it('两份都没写 → 两条问题', () => {
    expect(checkGate(EN, ZH)).toHaveLength(2)
  })

  /** 双语方案的头号风险:只认真写了一边。 */
  it('只写了英文一边 → 拦住', () => {
    const problems = checkGate(filled(EN, '## Unreleased'), ZH)
    expect(problems.join('\n')).toMatch(/zh-CN.*空/s)
  })

  it('版本序列不一致(漏改一边的版本节)→ 拦住', () => {
    const zhMissing = filled(ZH, '## 未发布').replace(/## v6\.813\.6 — 2026-08-13[\s\S]*/, '')
    const problems = checkGate(filled(EN, '## Unreleased'), zhMissing)
    expect(problems.join('\n')).toMatch(/版本序列不一致/)
  })

  it('日期对不上也算漂移', () => {
    const zhBadDate = filled(ZH, '## 未发布').replace('2026-08-17', '2026-08-18')
    expect(checkGate(filled(EN, '## Unreleased'), zhBadDate).join('\n')).toMatch(/版本序列不一致/)
  })

  it('一次报全所有问题,不是撞一个停一个', () => {
    const zhMissing = ZH.replace(/## v6\.813\.6 — 2026-08-13[\s\S]*/, '')
    expect(checkGate(EN, zhMissing).length).toBeGreaterThanOrEqual(3)
  })
})

describe('rotate', () => {
  const src = filledUnreleased()
  function filledUnreleased() {
    return EN.replace('## Unreleased\n', '## Unreleased\n\n### Added\n\n- 新东西\n')
  }

  it('未发布区变成版本节,顶部补回空的未发布区', () => {
    const out = rotate(src, 'en', '6.818.1', '2026-08-18')
    expect(out).toMatch(/## Unreleased\n\n## v6\.818\.1 — 2026-08-18/)
    expect(out).toMatch(/## v6\.818\.1 — 2026-08-18\n\n### Added\n\n- 新东西/)
    expect(hasUnreleasedContent(out, 'en')).toBe(false)
  })

  it('旧版本节一个字不动', () => {
    const out = rotate(src, 'en', '6.818.1', '2026-08-18')
    expect(out).toContain('## v6.817.1 — 2026-08-17')
    expect(out).toContain('## v6.813.6 — 2026-08-13')
    expect(sectionFor(out, 'en', '6.813.6')).toBe('### Fixed\n\n- 乙')
  })

  it('轮转后仍能过门禁的序列校验', () => {
    const en = rotate(src, 'en', '6.818.1', '2026-08-18')
    const zh = rotate(filledUnreleased().replace('## Unreleased', '## 未发布'), 'zh', '6.818.1', '2026-08-18')
    const seq = (t: string, lang: string) => parse(t, lang).versions.map((v: any) => v.version)
    expect(seq(en, 'en')).toEqual(seq(zh, 'zh'))
  })

  it('版本号或日期格式不对就抛,不写出坏文件', () => {
    expect(() => rotate(src, 'en', 'v6.818.1', '2026-08-18')).toThrow(/bad version/)
    expect(() => rotate(src, 'en', '6.818.1', '2026/08/18')).toThrow(/bad date/)
  })
})

describe('sectionFor', () => {
  it('取的是该版本那一节,不含标题', () => {
    expect(sectionFor(EN, 'en', '6.817.1')).toBe('### Added\n\n- 甲')
  })

  /** 取错段落 = 发出去的说明是上一个版本的,而且看起来完全正常。 */
  it('不会串到下一个版本节', () => {
    expect(sectionFor(EN, 'en', '6.817.1')).not.toContain('乙')
  })

  it('不会把「未发布」区当成版本节', () => {
    const withPending = EN.replace('## Unreleased\n', '## Unreleased\n\n- 还没发的\n')
    expect(sectionFor(withPending, 'en', '6.817.1')).not.toContain('还没发的')
  })

  it('最后一个版本节取到文件末尾', () => {
    expect(sectionFor(EN, 'en', '6.813.6')).toBe('### Fixed\n\n- 乙')
  })

  it('版本不存在时抛,而不是静默返回空正文', () => {
    expect(() => sectionFor(EN, 'en', '9.9.9')).toThrow(/no section/)
  })
})
