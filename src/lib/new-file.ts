// 新建文档的文本:正文 + OKF 概念文档头(§4.1 的必填 type)。
import { CONCEPT_TYPE, conceptFileText } from './okf/concept'

const H1 = /^#\s+(.+?)\s*$/m

/** 给一段新建正文补上 frontmatter;已有 frontmatter 的正文原样返回。 */
export function newFileText(body: string): string {
  if (body.startsWith('---\n')) return body
  const title = body.match(H1)?.[1]
  return conceptFileText({ type: CONCEPT_TYPE.note, title }, body)
}
