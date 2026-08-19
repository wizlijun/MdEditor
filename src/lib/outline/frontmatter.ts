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
 * 一件事:给已经成型的文本追加一个可能缺失的键。
 *
 * 纯文本变换,不做 I/O、不判断"是不是创建":调用方(OutlineEditor.svelte
 * 的 flushDisk)已经用 `!existed` 判定过创建时机,只在那个分支传 author;
 * 保存路径(store.svelte.ts)永远不会调用这个函数,读它就知道红线在哪。
 */
export function signCompanionNoteText(text: string, author?: ConceptMeta['generated']): string {
  if (!author) return text
  const { frontmatter, body } = splitFrontmatterBlock(text)
  if (frontmatter == null) return text
  const doc = parseDocument(frontmatter)
  if (doc.contents == null || !isMap(doc.contents) || doc.has('generated')) return text
  doc.set('generated', yamlSafeNode(doc, author))
  return `---\n${doc.toString().replace(/\n$/, '')}\n---\n${body}`
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
