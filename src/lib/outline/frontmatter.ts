// src/lib/outline/frontmatter.ts
import { parseDocument, isMap } from 'yaml'
import { CONCEPT_TYPE, touchConceptFrontmatter, yamlSafeNode, type ConceptMeta } from '../okf/concept'
import { normalize } from '../paths'

export interface TouchOpts {
  /** 缺 title 时写入的标题(原始标题,未 slug 化) */
  title: string
  /** 缺 type 时写入的 OKF 概念类型(§4.1 REQUIRED);默认 Outline Note */
  type?: string
  /** 缺 sources 时写入的来源(§5.1);镜像的伴生笔记用它记源文件路径 */
  sources?: ConceptMeta['sources']
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
  })
  const doc = parseDocument(filled)
  if (doc.contents == null || !isMap(doc.contents)) return raw ?? ''
  doc.set('updated', yamlSafeNode(doc, now))
  return doc.toString().replace(/\n$/, '')
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
