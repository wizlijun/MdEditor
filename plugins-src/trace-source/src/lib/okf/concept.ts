// Copied verbatim from `src/lib/okf/concept.ts` (the main app's OKF frontmatter
// production entry point) — DO NOT modify beyond this header. Trace Source runs
// in an isolated plugin webview that cannot `import` anything under the host
// repo's `src/` (see docs/plugin-v2-development.md §9.1: plugins must not
// depend on the main app's source tree), so the OKF writer surface has to be
// vendored into the plugin instead of shared by import. If the upstream file
// changes, port the change here too — this copy is expected to stay byte-for-
// byte identical to upstream except for this comment block and the
// `basename` import (the plugin has no `../paths` module, so it is inlined).
//
// OKF v0.2 概念文档的唯一 frontmatter 生产入口。
// 规范见 docs/okf-v0.2-format-constraints.md;整改计划见 docs/okf-v0.2-conformance-audit.md。
//
// 规则(§4.1):`type` 是唯一必填字段;生产者 MAY 加任意额外键,消费者往返时
// SHOULD 保留未知键 —— 所以这里只补缺失的键,已有键的值与顺序一律不动。
import { parseDocument, isMap, isScalar } from 'yaml'

const basename = (p: string): string => p.split('/').pop() ?? p

/**
 * 本项目使用的 `type` 取值表。OKF 不做中心注册(§4.1),但项目内必须唯一登记
 * 在这里,避免每个写入点各造一套。新增写入点时在此登记后再用。
 *
 * **新增一个值时,必须同时给 `searchidx` 的 origin 分级表(`searchidx/src/
 * origin.rs` 的 `mapped_type_origin`)补上对应档位**,并跑一次
 * `pnpm gen:origin-types` 重新生成 `searchidx/tests/fixtures/origin/
 * concept-types.json`(design spec §3.1)。`concept-origin-sync.test.ts` +
 * `origin.rs` 的跨语言测试只挡得住「加了没映射」——新值编译/测试都过但没人给它
 * 定档位——挡不住「映射到了错误的层」;后者要靠加值的人自己判断该进
 * human/derived/source 哪一档。
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
  /** AI 先读:电子书摘要 `YYYY-MM-DD-summary.md`(claude-agent ai-read-ebook 任务产出) */
  bookSummary: 'Book Summary',
  /** Reading Insights 的阅读数据报告 */
  readingReport: 'Reading Report',
  /** 决策日志:未决看板 / 已裁决归档 */
  decisionBoard: 'Decision Board',
  decisionArchive: 'Decision Archive',
  /** agent 写进 `answers/` 的长答案 */
  answer: 'Answer',
  /** 奇思妙想:用户写下的 idea 原文(plugins-src/idea-spark) */
  idea: 'Idea',
  /** 奇思妙想:agent 产出的论证文档 `<name>.proof.md` */
  ideaProof: 'Idea Proof',
  /** 溯源:agent 产出的溯源摘要 `<dir>/<ts>-source-trace.md`(trace-source 插件) */
  traceReport: 'Trace Report',
  /** 溯源:下载的原始材料全文(字幕转写/博客正文/论文节选),报告同名子目录 */
  traceMaterial: 'Trace Material',
  /** 溯源:用户的委托稿,报告同名子目录下的 `00-request.md`(委托时落盘,agent 只读) */
  traceRequest: 'Trace Request',
  /** vault 根的 AGENTS.md(模板见 src-tauri/templates/AGENTS.md) */
  vaultConventions: 'Vault Conventions',
} as const

/**
 * §8/§9 的保留文件名:`index.md` 是目录索引、`log.md` 是变更日志,
 * **MUST NOT** 用作概念文档。(校验器侧的同名常量在 scripts/okf-lint-core.mjs,
 * 那份是纯 JS、供 CLI 与测试共用,两处必须一起改。)
 */
export const RESERVED_CONCEPT_NAMES = ['index.md', 'log.md'] as const

/** 路径或文件名是否是保留名。 */
export function isReservedConceptName(pathOrName: string): boolean {
  const base = basename(pathOrName).toLowerCase()
  return (RESERVED_CONCEPT_NAMES as readonly string[]).includes(base)
}

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
    if (!doc.has(key)) doc.set(key, yamlSafeNode(doc, value))
  }
  return doc.toString().replace(/\n$/, '')
}

/**
 * YAML 1.1(PyYAML 等)会把 `2026-07-10` 读成 date、`yes`/`no` 读成 bool,而
 * YAML 1.2(本项目用的 js-yaml 家族)读成字符串。日记的 title 就是日期字符串,
 * 跨解析器语义漂移会让 agent 拿到 date 对象 —— 这类标量加引号钉成字符串。
 * 完整时间戳(`generated.at` / `created`)不在此列:那本来就是时间,读成时间没错,
 * 而且加引号会让既有文件的 diff 全线翻动。
 */
const YAML11_AMBIGUOUS = /^(\d{4}-\d{1,2}-\d{1,2}|y|Y|yes|Yes|YES|n|N|no|No|NO|on|On|ON|off|Off|OFF)$/

export function yamlSafeNode(doc: ReturnType<typeof parseDocument>, value: unknown): unknown {
  if (typeof value !== 'string' || !YAML11_AMBIGUOUS.test(value)) return value
  const node = doc.createNode(value)
  if (isScalar(node)) node.type = 'QUOTE_DOUBLE'
  return node
}

/** 完整的概念文档文本:frontmatter + body。 */
export function conceptFileText(meta: ConceptMeta, body: string): string {
  return `---\n${touchConceptFrontmatter(null, meta)}\n---\n${body}`
}
