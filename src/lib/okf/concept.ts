// OKF v0.2 概念文档的唯一 frontmatter 生产入口。
// 规范见 docs/okf-v0.2-format-constraints.md;整改计划见 docs/okf-v0.2-conformance-audit.md。
//
// 规则(§4.1):`type` 是唯一必填字段;生产者 MAY 加任意额外键,消费者往返时
// SHOULD 保留未知键 —— 所以这里只补缺失的键,已有键的值与顺序一律不动。
import { parseDocument, isMap } from 'yaml'

/**
 * 本项目使用的 `type` 取值表。OKF 不做中心注册(§4.1),但项目内必须唯一登记
 * 在这里,避免每个写入点各造一套。新增写入点时在此登记后再用。
 */
export const CONCEPT_TYPE = {
  /** 普通 markdown 笔记(⌘N 新建、vault 外建页) */
  note: 'Note',
  /** 伴生/大纲笔记 `.note.md` */
  outlineNote: 'Outline Note',
  /** 日期笔记 `dailynote/yyyy/yyyy-MM-dd.note.md` */
  dailyNote: 'Daily Note',
  /** wikilink 页 `wikipage/<title>.note.md` */
  wikiPage: 'Wiki Page',
  /** 电子书导入产出的 `book.md` */
  book: 'Book',
  /** Reading Insights 的阅读数据报告 */
  readingReport: 'Reading Report',
} as const

/** OKF actor(§7)。 */
export interface Actor {
  by: string
  at: string
}

export interface ConceptMeta {
  /** REQUIRED(§4.1) */
  type: string
  title?: string
  description?: string
  /** 唯一标识底层资产的 URI(§4.1) */
  resource?: string
  tags?: string[]
  /** 生成署名(§5.2) */
  generated?: Actor
  /** 验证事件(§5.2);单条也可写成裸 mapping */
  verified?: Actor | Actor[]
  /** 来源与可信度信号(§5.1) */
  sources?: Array<{
    resource: string
    id?: string
    title?: string
    author?: string
    last_modified?: string
  }>
  /** 生命周期(§5.4);缺省即 stable,故通常不必写 */
  status?: 'draft' | 'stable' | 'deprecated'
  /** 过期日期 `YYYY-MM-DD`(§5.4) */
  stale_after?: string
  /** §4.1:生产者 MAY 加入任意额外键 */
  [extra: string]: unknown
}

/**
 * 在既有 frontmatter(`---` 之间的内容,不含分隔符)上补齐缺失的 OKF 字段,
 * 返回新的 frontmatter 文本。已有键不覆盖、顺序不变、未知键原样保留;
 * frontmatter 不是 mapping 时原样返回,绝不做破坏性改写。
 */
export function touchConceptFrontmatter(raw: string | null, meta: ConceptMeta): string {
  const doc = parseDocument(raw ?? '')
  if (doc.contents == null) doc.contents = doc.createNode({}) as never
  else if (!isMap(doc.contents)) return raw ?? ''
  for (const [key, value] of Object.entries(meta)) {
    if (value === undefined) continue
    if (!doc.has(key)) doc.set(key, value)
  }
  return doc.toString().replace(/\n$/, '')
}

/** 完整的概念文档文本:frontmatter + body。 */
export function conceptFileText(meta: ConceptMeta, body: string): string {
  return `---\n${touchConceptFrontmatter(null, meta)}\n---\n${body}`
}
