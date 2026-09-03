// 新建文档的文本:正文 + OKF 概念文档头(§4.1 的必填 type)。
// `author` 在场时写 §5.2 的 `generated` —— 这是「人写的、是谁写的」在元数据层
// 的唯一表达(spec: docs/superpowers/specs/2026-08-20-human-authorship-signature-design.md)。
// 拿不到身份时**不签**:一个缺席的署名是诚实的,一个编出来的不是。
import { CONCEPT_TYPE, conceptFileText, type ConceptMeta } from './okf/concept'

const H1 = /^#\s+(.+?)\s*$/m

/** 给一段新建正文补上 frontmatter;已有 frontmatter 的正文原样返回。 */
export function newFileText(body: string, author?: ConceptMeta['generated']): string {
  if (body.startsWith('---\n')) return body
  const title = body.match(H1)?.[1]
  return conceptFileText({ type: CONCEPT_TYPE.note, title, generated: author }, body)
}
