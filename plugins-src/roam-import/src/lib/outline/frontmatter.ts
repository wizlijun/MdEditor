// src/lib/outline/frontmatter.ts — copied verbatim from host
// src/lib/outline/frontmatter.ts (only touchFrontmatter is used by convert).
import { parseDocument, isMap } from 'yaml'

export interface TouchOpts {
  /** 缺 title 时写入的标题(原始标题,未 slug 化) */
  title: string
  /** 缺 type 时写入的 OKF 概念类型(§4.1 REQUIRED);默认 Outline Note */
  type?: string
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
 * updated 总是刷新。
 * 未知键(如 roam-uid)与既有键顺序保留。非 mapping 的 front-matter
 * 原样返回,不做破坏性改写。
 */
export function touchFrontmatter(raw: string | null, opts: TouchOpts): string {
  const now = opts.now ?? new Date().toISOString()
  const doc = parseDocument(raw ?? '')
  if (doc.contents == null) doc.contents = doc.createNode({}) as never
  else if (!isMap(doc.contents)) return raw ?? ''
  if (!doc.has('type')) doc.set('type', opts.type ?? 'Outline Note')
  if (!doc.has('title')) doc.set('title', opts.title)
  if (!doc.has('created')) doc.set('created', opts.created ?? now)
  doc.set('updated', now)
  return doc.toString().replace(/\n$/, '')
}
