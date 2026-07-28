// src/lib/note-anno/answers-store.svelte.ts
// 正文内联答复卡片的数据源:只为「当前活动主文档」按需加载其配套 .note.md。
// 不预加载整个 vault(懒加载第一层);答复正文的 markdown 渲染在卡片展开时才做(第二层)。
import { parseOutline } from '../outline/markdown'
import { deriveAnswers, answeredByNoteText, type AnswerEntry } from '../outline/answers'
import { outline, serializeDoc } from '../outline/store.svelte'
import { noteHomeForRead } from '../outline/note-home'

interface AnswersState {
  /** 索引来源的 .note.md 路径(采纳回写要用) */
  notePath: string | null
  entries: AnswerEntry[]
  /** 每次重建自增:宿主据此让 PM 插件重建卡片 decoration */
  version: number
}

export const answersStore = $state<AnswersState>({ notePath: null, entries: [], version: 0 })

/** 批注文本 → 待处理答复(只含 answered) */
export function answeredMap(): Map<string, AnswerEntry> {
  return answeredByNoteText(answersStore.entries)
}

/** 用一段 .note.md 文本重建索引(纯内存,供测试与内存树路径复用) */
export function setAnswersFromText(notePath: string, text: string): void {
  answersStore.notePath = notePath
  answersStore.entries = deriveAnswers(parseOutline(text))
  answersStore.version++
}

export function clearAnswers(): void {
  answersStore.notePath = null
  answersStore.entries = []
  answersStore.version++
}

/**
 * 为某个主文档加载答复索引。大纲已挂载同一 note 时直接用内存树(单一事实源,
 * 避免读到过期的盘上内容);否则按需读盘。任何失败都静默清空——卡片是增强,不是必需。
 */
export async function loadAnswersFor(mainPath: string | null | undefined): Promise<void> {
  if (!mainPath || !/\.md$/i.test(mainPath) || /\.notes?\.md$/i.test(mainPath)) { clearAnswers(); return }
  try {
    const { sotvaultStore } = await import('../sotvault.svelte')
    const notePath = noteHomeForRead(mainPath, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
    })
    if (!notePath) { clearAnswers(); return }
    if (outline.docPath === notePath) { setAnswersFromText(notePath, serializeDoc(false)); return }
    const fs = await import('@tauri-apps/plugin-fs')
    if (!(await fs.exists(notePath).catch(() => false))) { clearAnswers(); return }
    const text = await fs.readTextFile(notePath).catch(() => null)
    if (text == null) { clearAnswers(); return }
    setAnswersFromText(notePath, text)
  } catch (e) {
    console.warn('[answers] load failed:', e)
    clearAnswers()
  }
}
