// src/lib/note-anno/answer-sites.ts
// 算出「答复卡片挂在哪」:按批注文本(annotation mark / note_anchor 的 note 属性)
// 与答复索引匹配,把卡片锚在该批注所在顶层块之后。
// 不用 line:: —— 行号随编辑漂移；以批注文本 + 同文本出现序号按文档顺序一一匹配。
import type { Node as PMNode } from 'prosemirror-model'
import type { AnswerEntry, AnswerIndex } from '../outline/answers'

export interface CardSite {
  /** 文档位置:该批注所在顶层块之后 */
  pos: number
  entry: AnswerEntry
}

export function collectCardSites(doc: PMNode, entries: AnswerIndex): CardSite[] {
  if (entries.size === 0) return []
  const sites: CardSite[] = []
  const occurrences = new Map<string, number>()
  doc.descendants((node, pos) => {
    let note: string | null = null
    let end = pos
    if (node.isText) {
      const mark = node.marks.find(m => m.type.name === 'annotation')
      if (!mark) return
      end = pos + node.nodeSize
      // 同一条批注跨多个文本片段(如中间有加粗)时只在末段出一次
      const after = doc.resolve(end).nodeAfter
      if (after && mark.isInSet(after.marks)) return
      note = mark.attrs.note as string
    } else if (node.type.name === 'note_anchor') {
      note = node.attrs.note as string
      end = pos + node.nodeSize
    }
    if (note == null) return
    const occurrence = occurrences.get(note) ?? 0
    occurrences.set(note, occurrence + 1)
    const entry = entries.get(note)?.[occurrence]
    if (!entry) return
    const $end = doc.resolve(end)
    const insertPos = $end.depth >= 1 ? $end.after(1) : end
    sites.push({ pos: insertPos, entry })
  })
  return sites
}
