// src/lib/outline/answers.ts
// 从 .note.md 树派生「答复索引」:供正文内联卡片按批注文本锚定。
// 纯函数,无 IO —— 懒加载与缓存在 note-anno/answers-store 里。
import { childrenOf, answerBodyOf, type OutlineTree, type QuestionStatus } from './model'

export interface AnswerEntry {
  /** question 节点内容 = 源文档里那条批注的文本(锚定用的稳定身份) */
  noteText: string
  status: QuestionStatus
  /** 剥掉围栏的答复 markdown */
  body: string
  by?: string
  answeredAt?: string
  /** 采纳后回写 status:: adopted 用 */
  questionId: string
}

/** 树里每个「带答复节点的 question」派生一条索引 */
export function deriveAnswers(tree: OutlineTree): AnswerEntry[] {
  const out: AnswerEntry[] = []
  for (const q of tree.nodes.values()) {
    if (q.source !== 'question') continue
    const a = childrenOf(tree, q.id).find(c => c.source === 'answer')
    if (!a) continue
    out.push({
      noteText: q.content,
      status: q.status ?? 'open',
      body: answerBodyOf(a),
      by: a.answeredBy,
      answeredAt: a.answeredAt,
      questionId: q.id,
    })
  }
  return out
}

/** 批注文本 → 待你处理的答复。只含 answered:open 无答复、adopted/closed 已了结。 */
export function answeredByNoteText(entries: AnswerEntry[]): Map<string, AnswerEntry> {
  const m = new Map<string, AnswerEntry>()
  for (const e of entries) if (e.status === 'answered') m.set(e.noteText, e)
  return m
}
