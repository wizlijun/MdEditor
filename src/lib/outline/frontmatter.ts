// src/lib/outline/frontmatter.ts
import { parseDocument, isMap } from 'yaml'
import { CONCEPT_TYPE, touchConceptFrontmatter, yamlSafeNode, type ConceptMeta } from '../okf/concept'
import { normalize } from '../paths'
import { splitFrontmatterBlock } from './markdown'

export interface TouchOpts {
  /** 缺 title 时写入的标题(原始标题,未 slug 化) */
  title: string
  /** 缺 type 时写入的 OKF 概念类型(§4.1 REQUIRED);默认 Outline Note */
  type?: string
  /** 缺 sources 时写入的来源(§5.1);镜像的伴生笔记用它记源文件路径 */
  sources?: ConceptMeta['sources']
  /** 缺 generated 时写入的生成署名(§5.2)。**只有创建路径才传** ——
   *  保存路径传了会让旧文件凭一次保存就获得人写署名(spec §2.2)。 */
  generated?: ConceptMeta['generated']
  /** 缺 created 时的回退值(通常取文件 birthtime);不传用 now */
  created?: string
  /** 注入时间,便于测试;默认当前时间 ISO 8601 */
  now?: string
}

/** front-matter 是否含顶层键(raw 为 --- 分隔符之间的内容,不含分隔符) */
export function fmHas(raw: string | null, key: string): boolean {
  if (!raw) return false
  const doc = parseDocument(raw)
  return doc.contents != null && isMap(doc.contents) && doc.has(key)
}

/**
 * 补齐/刷新 front-matter:type(OKF §4.1 必填)、title、created 缺失时补上,
 * updated 总是刷新。未知键(如 roam-uid)与既有键顺序保留。非 mapping 的
 * front-matter 原样返回,不做破坏性改写。
 */
export function touchFrontmatter(raw: string | null, opts: TouchOpts): string {
  const now = opts.now ?? new Date().toISOString()
  const filled = touchConceptFrontmatter(raw, {
    type: opts.type ?? CONCEPT_TYPE.outlineNote,
    title: opts.title,
    created: opts.created ?? now,
    sources: opts.sources,
    generated: opts.generated,
  })
  const doc = parseDocument(filled)
  if (doc.contents == null || !isMap(doc.contents)) return raw ?? ''
  doc.set('updated', yamlSafeNode(doc, now))
  return doc.toString().replace(/\n$/, '')
}

/**
 * 伴生 `.note.md` **首次落盘**这一刻的签名 patch(spec:companion note 也是
 * 一个创建入口)。只在 `generated` 键缺失时补一条,其余字段(type/title/
 * created/updated/...)一律不动——这不是又一个 touchFrontmatter,它只做
 * 一件事:给已经成型的 frontmatter 追加一个可能缺失的键。`raw` 是不含 `---`
 * 分隔符的 frontmatter 内容,与 `OutlineTree.frontmatter` 同一种形状——
 * `signOutlineFrontmatterOnCreate`(store.svelte.ts)直接拿它 patch 内存树,
 * `signCompanionNoteText` 拿它 patch 落盘的完整文本,两处用的是同一次 patch
 * 逻辑,不会出现"盘上有、内存没有"的分叉。
 *
 * 纯文本变换,不做 I/O、不判断"是不是创建":调用方已经用 `!existed` 判定过
 * 创建时机,只在那个分支传 author;保存路径(store.svelte.ts 的两处
 * touchFrontmatter 调用)永远不会调用这个函数,读它就知道红线在哪。
 */
export function signFrontmatterBlock(raw: string | null, author?: ConceptMeta['generated']): string | null {
  if (!author || raw == null) return raw
  const doc = parseDocument(raw)
  if (doc.contents == null || !isMap(doc.contents) || doc.has('generated')) return raw
  doc.set('generated', yamlSafeNode(doc, author))
  return doc.toString().replace(/\n$/, '')
}

/** `signFrontmatterBlock` 的完整文本(frontmatter + body)版本。 */
export function signCompanionNoteText(text: string, author?: ConceptMeta['generated']): string {
  const { frontmatter, body } = splitFrontmatterBlock(text)
  const signed = signFrontmatterBlock(frontmatter, author)
  if (signed == null || signed === frontmatter) return text
  return `---\n${signed}\n---\n${body}`
}

/**
 * 大纲笔记的 OKF 概念类型:按 vault 约定目录判定(§4.1 的 type 用于「按类型
 * 发现概念」),目录名可配置故由调用方传入。
 */
export function outlineConceptType(
  path: string,
  dirs: { wikipage: string; dailynote: string },
): string {
  const segments = normalize(path).split('/').slice(0, -1)
  if (segments.includes(dirs.dailynote)) return CONCEPT_TYPE.dailyNote
  if (segments.includes(dirs.wikipage)) return CONCEPT_TYPE.wikiPage
  return CONCEPT_TYPE.outlineNote
}
