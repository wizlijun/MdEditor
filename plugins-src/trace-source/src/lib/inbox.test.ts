// inbox.ts 的合同:哪些文件算报告、按什么顺序列、标题从哪来、删除带走什么、
// 委托稿长什么样、没长成报告的委托怎么被发现。
import { describe, expect, it, vi } from 'vitest'
import {
  buildRequestDoc,
  createdFromName,
  deleteReport,
  documentPathFor,
  listReports,
  materialsDirFor,
  previewDelete,
  relativeAge,
  REPORT_SUFFIX,
  requestPathFor,
  stripFrontmatter,
} from './inbox'

const DIR = 'inbox/traces'

function io(
  files: Record<string, string>,
  dirs: Record<string, string[]> = {},
  /** 目录下的深层文件,键是相对 DIR 的路径(如 `x-source-trace/00-request.md`)。 */
  deep: Record<string, string> = {},
) {
  return {
    list: vi.fn(async (path: string) => {
      if (path === DIR) {
        const entries = [
          ...Object.keys(files).map((n) => ({ name: n, is_dir: false })),
          ...Object.keys(dirs).map((n) => ({ name: n, is_dir: true })),
        ]
        return { entries }
      }
      const sub = dirs[path.split('/').pop() ?? '']
      if (sub) return { entries: sub.map((n) => ({ name: n, is_dir: false })) }
      throw new Error('no such dir')
    }),
    read: vi.fn(async (path: string) => {
      const rel = path.startsWith(`${DIR}/`) ? path.slice(DIR.length + 1) : path
      if (rel in deep) return { content: deep[rel] }
      const name = path.split('/').pop() ?? ''
      if (name in files) return { content: files[name] }
      throw new Error('no such file')
    }),
    remove: vi.fn(async (_path: string) => ({ ok: true as const })),
  }
}

describe('listReports', () => {
  it('只认 -source-trace.md 结尾的文件,倒序,标题取 frontmatter title,hasReport=true', async () => {
    const reports = await listReports(
      io({
        '2026-08-17-090000-source-trace.md': '---\ntitle: 早的\n---\n正文',
        '2026-08-18-143012-source-trace.md': '---\ntype: Trace Report\ntitle: 晚的\n---\n正文',
        'not-a-report.md': '---\ntitle: 闲杂\n---',
        '2026-08-18-150000-source-trace.txt': 'wrong ext',
      }),
      DIR,
    )
    expect(reports.map((r) => r.name)).toEqual([
      '2026-08-18-143012-source-trace.md',
      '2026-08-17-090000-source-trace.md',
    ])
    expect(reports.map((r) => r.title)).toEqual(['晚的', '早的'])
    expect(reports.every((r) => r.hasReport)).toBe(true)
  })

  it('frontmatter 缺失或读失败时 title 为 null,行仍在', async () => {
    const reports = await listReports(
      io({
        '2026-08-18-143012-source-trace.md': '没有 frontmatter 的正文',
      }),
      DIR,
    )
    expect(reports).toHaveLength(1)
    expect(reports[0].title).toBeNull()
  })

  it('目录读不了 → 抛出(调用方据此显示「读取失败」而非「还没有」)', async () => {
    const bad = io({})
    bad.list.mockRejectedValue(new Error('io'))
    await expect(listReports(bad, DIR)).rejects.toThrow()
  })

  it('有委托稿目录但无报告 → 孤儿行(hasReport=false,标题读自 00-request.md),排序仍按时间倒序', async () => {
    const rows = await listReports(
      io(
        { '2026-08-17-090000-source-trace.md': '---\ntitle: 完成的\n---' },
        {
          '2026-08-18-143012-source-trace': ['00-request.md'],
          // 已有报告的材料目录不再重复成行
          '2026-08-17-090000-source-trace': ['00-request.md', '01-blog.md'],
        },
        { '2026-08-18-143012-source-trace/00-request.md': '---\ntype: Trace Request\ntitle: 悬着的\n---\n> 引文' },
      ),
      DIR,
    )
    expect(rows.map((r) => [r.name, r.hasReport, r.title])).toEqual([
      ['2026-08-18-143012-source-trace.md', false, '悬着的'],
      ['2026-08-17-090000-source-trace.md', true, '完成的'],
    ])
  })
})

describe('materialsDirFor / createdFromName', () => {
  it('材料目录 = 报告名去 .md', () => {
    expect(materialsDirFor('2026-08-18-143012-source-trace.md')).toBe('2026-08-18-143012-source-trace')
  })

  it('从报告名解出创建时刻;不合规名字给 null', () => {
    const d = createdFromName('2026-08-18-143012-source-trace.md')
    expect(d?.getFullYear()).toBe(2026)
    expect(d?.getHours()).toBe(14)
    expect(d?.getSeconds()).toBe(12)
    expect(createdFromName('renamed.md')).toBeNull()
    expect(createdFromName('2026-13-99-999999-source-trace.md')).toBeNull()
  })

  it('relativeAge 分档:分钟/小时/天', () => {
    const now = new Date(2026, 7, 18, 15, 0, 0)
    expect(relativeAge(new Date(2026, 7, 18, 14, 30, 0), now)).toEqual({ value: -30, unit: 'minute' })
    expect(relativeAge(new Date(2026, 7, 18, 10, 0, 0), now)).toEqual({ value: -5, unit: 'hour' })
    expect(relativeAge(new Date(2026, 7, 10, 15, 0, 0), now)).toEqual({ value: -8, unit: 'day' })
  })
})

describe('previewDelete / deleteReport', () => {
  const name = '2026-08-18-143012-source-trace.md'

  it('预览列出报告与每份材料;删除逐个 remove(host 拒删目录)', async () => {
    const h = io(
      { [name]: '---\ntitle: x\n---' },
      { '2026-08-18-143012-source-trace': ['01-blog.md', '02-subs.md'] },
    )
    const lines = await previewDelete(h, DIR, name)
    expect(lines).toEqual([
      `${DIR}/${name}`,
      `${DIR}/2026-08-18-143012-source-trace/01-blog.md`,
      `${DIR}/2026-08-18-143012-source-trace/02-subs.md`,
    ])
    await deleteReport(h, DIR, name)
    const removed = h.remove.mock.calls.map(([p]) => p)
    expect(removed).toEqual(lines)
  })

  it('没有材料目录时只删报告,不报错', async () => {
    const h = io({ [name]: 'x' })
    expect(await previewDelete(h, DIR, name)).toEqual([`${DIR}/${name}`])
    await deleteReport(h, DIR, name)
    expect(h.remove.mock.calls.map(([p]) => p)).toEqual([`${DIR}/${name}`])
  })
})

describe('constants', () => {
  it('报告后缀是产品约定,钉死', () => {
    expect(REPORT_SUFFIX).toBe('-source-trace.md')
  })

  it('委托稿住在材料目录里,00 号——材料从 01 起,永不相撞', () => {
    expect(requestPathFor('2026-08-18-143012-source-trace.md')).toBe(
      '2026-08-18-143012-source-trace/00-request.md',
    )
  })

  it('Inbox 行在插件自身编辑区打开正确文档:完成项是报告,未完成项是委托稿', () => {
    const name = '2026-08-18-143012-source-trace.md'
    expect(documentPathFor(DIR, { name, hasReport: true })).toBe(`${DIR}/${name}`)
    expect(documentPathFor(DIR, { name, hasReport: false })).toBe(
      `${DIR}/2026-08-18-143012-source-trace/00-request.md`,
    )
  })
})

describe('buildRequestDoc', () => {
  it('frontmatter 带 type: Trace Request 与从首行截取的 title,正文原样', () => {
    const doc = buildRequestDoc('> 这段话是谁说的\n> 第二行\n\nSource-Doc: a.md\n')
    expect(doc).toMatch(/^---\n/)
    expect(doc).toContain('type: Trace Request')
    expect(doc).toContain('title: 这段话是谁说的')
    expect(doc).toContain('> 这段话是谁说的\n> 第二行\n\nSource-Doc: a.md')
  })

  it('title 去掉引用符、截断超长行、对引号安全', () => {
    const doc = buildRequestDoc(`> ${'长'.repeat(80)}\n`)
    const m = /title: (.+)/.exec(doc)!
    expect(m[1].length).toBeLessThanOrEqual(61) // 60 + 省略号
    expect(buildRequestDoc('> a "quoted" line\n')).toContain('title: a "quoted" line')
    // 冒号开头等 YAML 险字符走引号包裹
    expect(buildRequestDoc(': tricky\n')).toContain('title: ": tricky"')
  })

  it('空文本也给出非空 title(OKF 生产者约束:type 非空、文档可解析)', () => {
    const doc = buildRequestDoc('   \n')
    expect(doc).toMatch(/title: \S/)
  })
})

describe('stripFrontmatter', () => {
  it('去掉 frontmatter 还原委托正文;没有 frontmatter 原样返回', () => {
    expect(stripFrontmatter('---\ntype: Trace Request\ntitle: x\n---\n\n> 正文\n')).toBe('> 正文\n')
    expect(stripFrontmatter('> 没有头\n')).toBe('> 没有头\n')
  })
})
