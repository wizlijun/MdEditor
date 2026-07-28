// src/lib/note-anno/answer-sites.ts
// 算出「答复卡片挂在哪」:按批注文本(annotation mark / note_anchor 的 note 属性)
// 与答复索引匹配,把卡片锚在该批注所在顶层块之后。
// 不用 line:: —— 行号随编辑漂移,批注文本才是 sync 也在用的稳定身份。
import type { Node as PMNode } from 'prosemirror-model'
import type { AnswerEntry } from '../outline/answers'

export interface CardSite {
  /** 文档位置:该批注所在顶层块之后 */
  pos: number
  entry: AnswerEntry
}

export function collectCardSites(doc: PMNode, entries: Map<string, AnswerEntry>): CardSite[] {
  if (entries.size === 0) return []
  const sites: CardSite[] = []
  const seen = new Set<string>()
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
    const entry = entries.get(note)
    if (!entry) return
    const $end = doc.resolve(end)
    const insertPos = $end.depth >= 1 ? $end.after(1) : end
    const key = `${insertPos}\n${note}`
    if (seen.has(key)) return
    seen.add(key)
    sites.push({ pos: insertPos, entry })
  })
  return sites
}
