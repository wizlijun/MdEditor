// src/lib/note-anno/adopt-answer.ts
// 「采纳入正文」——本特性里唯一会改源 .md 的动作,且只由人点击触发。
// 插入的是干净 markdown(无 ✦、无出处、无隐形标记):你确认过,它就是你的正文。
import type { EditorView } from 'prosemirror-view'
import { parseMarkdown } from '@moraya/core'
import { parseOutline, serializeOutline } from '../outline/markdown'
import { addVerified } from '../okf/actor'
import type { AnswerEntry } from '../outline/answers'
import { answersStore, loadAnswersFor } from './answers-store.svelte'

/**
 * 在一段 .note.md 文本里把指定问题置为 adopted。找不到该问题返回 null(不写盘)。
 * 传 verifier 时同时把这次人工确认写进 front-matter 的 `verified`(OKF §5.2)——
 * 「你确认过的判断」是 vault 里最有价值的数据,不能只剩一个枚举值。
 */
export function markAdoptedInText(
  noteText: string,
  questionContent: string,
  verifier?: { by: string; at: string },
): string | null {
  const tree = parseOutline(noteText)
  const q = [...tree.nodes.values()].find(n => n.source === 'question' && n.content === questionContent)
  if (!q) return null
  q.status = 'adopted'
  if (verifier) tree.frontmatter = addVerified(tree.frontmatter, verifier.by, verifier.at)
  return serializeOutline(tree)
}

/**
 * 把答复插入到锚点块之后,包成引用块(`>`)——采纳进来的内容要一眼看出是后续补充,
 * 与你原本的正文区分开。仍是干净 markdown:没有 ✦、没有出处标记,只是引用格式。
 * 一次 transaction,⌘Z 即回退。
 */
export function insertAnswerIntoDoc(view: EditorView, entry: AnswerEntry, pos: number): void {
  const parsed = parseMarkdown(entry.body, view.state.schema)
  const quote = view.state.schema.nodes.blockquote
  // 用 schema 节点包裹而不是给每行加 "> ":嵌套列表/代码块也能正确成块
  const content = quote ? quote.create(null, parsed.content) : parsed.content
  view.dispatch(view.state.tr.insert(pos, content).scrollIntoView())
}

/** 回写 .note.md 的 status:: adopted + front-matter 的 verified(人工确认署名)。
 *  大纲挂载着同一 note 时改内存树,否则读改写盘。 */
export async function markAdoptedOnDisk(entry: AnswerEntry): Promise<void> {
  const notePath = answersStore.notePath
  if (!notePath) return
  try {
    const { humanActor } = await import('../okf/identity')
    const verifier = { by: await humanActor(), at: new Date().toISOString() }
    const { outline, bump, markDirty } = await import('../outline/store.svelte')
    if (outline.docPath === notePath) {
      const q = outline.tree.nodes.get(entry.questionId)
        ?? [...outline.tree.nodes.values()].find(n => n.source === 'question' && n.content === entry.noteText)
      if (q) {
        q.status = 'adopted'
        outline.tree.frontmatter = addVerified(outline.tree.frontmatter, verifier.by, verifier.at)
        bump(); markDirty()
      }
      return
    }
    const fs = await import('@tauri-apps/plugin-fs')
    const disk = await fs.readTextFile(notePath).catch(() => null)
    if (disk == null) return
    // 保真防线:采纳会把整份 .note.md 重新解析+序列化写回。若解析器读不「懂」这份文件
    // (最常见是 agent 把答复围栏写短了,嵌套 ``` 提前闭合),重写会重塑文件结构、
    // 毁掉 type:: answer 标记。读不懂就不写——绝不重塑一份我们没完全理解的文件。
    if (serializeOutline(parseOutline(disk)) !== disk) {
      console.warn('[adopt] companion note does not round-trip; skipping status writeback')
      return
    }
    const next = markAdoptedInText(disk, entry.noteText, verifier)
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
