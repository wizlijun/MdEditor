// src/lib/note-anno/adopt-answer.ts
// 「采纳入正文」——本特性里唯一会改源 .md 的动作,且只由人点击触发。
// 插入的是干净 markdown(无 ✦、无出处、无隐形标记):你确认过,它就是你的正文。
import type { EditorView } from 'prosemirror-view'
import { parseMarkdown } from '@moraya/core'
import { parseOutline, serializeOutline } from '../outline/markdown'
import type { AnswerEntry } from '../outline/answers'
import { answersStore, loadAnswersFor } from './answers-store.svelte'

/** 在一段 .note.md 文本里把指定问题置为 adopted。找不到该问题返回 null(不写盘)。 */
export function markAdoptedInText(noteText: string, questionContent: string): string | null {
  const tree = parseOutline(noteText)
  const q = [...tree.nodes.values()].find(n => n.source === 'question' && n.content === questionContent)
  if (!q) return null
  q.status = 'adopted'
  return serializeOutline(tree)
}

/** 把答复 markdown 作为干净正文插入到锚点块之后:一次 transaction,⌘Z 即回退。 */
export function insertAnswerIntoDoc(view: EditorView, entry: AnswerEntry, pos: number): void {
  const parsed = parseMarkdown(entry.body, view.state.schema)
  view.dispatch(view.state.tr.insert(pos, parsed.content).scrollIntoView())
}

/** 回写 .note.md 的 status:: adopted。大纲挂载着同一 note 时改内存树,否则读改写盘。 */
export async function markAdoptedOnDisk(entry: AnswerEntry): Promise<void> {
  const notePath = answersStore.notePath
  if (!notePath) return
  try {
    const { outline, bump, markDirty } = await import('../outline/store.svelte')
    if (outline.docPath === notePath) {
      const q = outline.tree.nodes.get(entry.questionId)
        ?? [...outline.tree.nodes.values()].find(n => n.source === 'question' && n.content === entry.noteText)
      if (q) { q.status = 'adopted'; bump(); markDirty() }
      return
    }
    const fs = await import('@tauri-apps/plugin-fs')
    const disk = await fs.readTextFile(notePath).catch(() => null)
    if (disk == null) return
    const next = markAdoptedInText(disk, entry.noteText)
    if (next == null || next === disk) return
    await fs.writeTextFile(notePath, next)
  } catch (e) {
    console.warn('[adopt] writeback failed:', e)
  }
}

/** 卡片按钮入口:插正文 → 回写状态 → 刷新索引(卡片随即消失)。 */
export async function adoptAnswer(
  view: EditorView, entry: AnswerEntry, pos: number, mainPath: string | null,
): Promise<void> {
  insertAnswerIntoDoc(view, entry, pos)
  await markAdoptedOnDisk(entry)
  await loadAnswersFor(mainPath)
}
