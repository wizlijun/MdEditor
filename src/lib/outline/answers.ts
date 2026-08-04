// src/lib/outline/answers.ts
// 从 .note.md 树派生「答复索引」:供正文内联卡片按批注文本锚定。
// 纯函数,无 IO —— 懒加载与缓存在 note-anno/answers-store 里。
import {
  childrenOf, answerBodyOf, stripAnswerSigil,
  type OutlineNode, type OutlineTree, type QuestionStatus,
} from './model'

export interface AnswerEntry {
  /** question 节点内容 = 源文档里那条批注的文本(锚定用的稳定身份) */
  noteText: string
  status: QuestionStatus
  /** 答复 markdown:剥掉围栏的正文 + 子节点大纲渲染成的嵌套列表 */
  body: string
  by?: string
  answeredAt?: string
  /** 采纳后回写 status:: adopted 用 */
  questionId: string
}

/**
 * 答复渲染成一段 markdown。两种形态都要认:
 *  - **单节点**:整段正文包在围栏里(`answerBodyOf` 剥壳即可);
 *  - **多节点**:答复节点写结论,论点分列成子节点大纲 —— 子树渲染成嵌套无序列表,
 *    卡片里就是一份层级清单。只取答复节点的首个子节点等于丢掉大半答复。
 *
 * 每个节点各自剥 `✦`:sigil 由展示端出(卡片头已有一个),采纳进正文要的是干净
 * markdown。
 */
export function answerMarkdownOf(tree: OutlineTree, answer: OutlineNode): string {
  const lead = stripAnswerSigil(answerBodyOf(answer)).trimEnd()
  const list: string[] = []
  const walk = (parentId: string, depth: number) => {
    for (const child of childrenOf(tree, parentId)) {
      const indent = '  '.repeat(depth)
      const [first = '', ...rest] = stripAnswerSigil(child.content).split('\n')
      list.push(`${indent}- ${first}`)
      // 续行对齐到条目内容列(`- ` 占两格);空行原样保留,它是段落分隔
      for (const line of rest) list.push(line === '' ? '' : `${indent}  ${line}`)
      walk(child.id, depth + 1)
    }
  }
  walk(answer.id, 0)
  if (list.length === 0) return lead
  // 列表前留空行:导语是段落,紧贴着写会被部分解析器并进同一段
  return lead ? `${lead}\n\n${list.join('\n')}` : list.join('\n')
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
      body: answerMarkdownOf(tree, a),
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
