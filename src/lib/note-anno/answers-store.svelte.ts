// src/lib/note-anno/answers-store.svelte.ts
// 正文内联答复卡片的数据源:只为「当前活动主文档」按需加载其配套 .note.md。
// 不预加载整个 vault(懒加载第一层);答复正文的 markdown 渲染在卡片展开时才做(第二层)。
import { parseOutline } from '../outline/markdown'
import { deriveAnswers, answeredByNoteText, type AnswerEntry, type AnswerIndex } from '../outline/answers'
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

/** 批注文本 → 按同文本出现序号排列的待处理答复(只含 answered) */
export function answeredMap(): AnswerIndex {
  return answeredByNoteText(answersStore.entries)
}

/** 用一段 .note.md 文本重建索引(纯内存,供测试与内存树路径复用) */
export function setAnswersFromText(notePath: string, text: string): void {
  answersStore.notePath = notePath
  answersStore.entries = deriveAnswers(parseOutline(text))
  answersStore.version++
}

export function clearAnswers(): void {
  stopWatching()
  answersStore.notePath = null
  answersStore.entries = []
  answersStore.version++
}

// ---- 配套 .note.md 的变更监听 ----------------------------------------------
// 既有的 file-watcher 只盯 tab.filePath(主文档),不看伴生笔记。没有这一段,
// 「你正读着文档、agent 在后台写进答复」不会有任何反应,要切走再切回才出卡片。
let unwatch: (() => void) | null = null
let watched: string | null = null
let reloadTimer: ReturnType<typeof setTimeout> | null = null

function stopWatching(): void {
  if (reloadTimer) { clearTimeout(reloadTimer); reloadTimer = null }
  if (unwatch) { try { unwatch() } catch { /* watcher 已失效,忽略 */ } unwatch = null }
  watched = null
}

async function watchCompanion(notePath: string, mainPath: string): Promise<void> {
  if (watched === notePath) return
  stopWatching()
  watched = notePath
  try {
    const { watchImmediate } = await import('@tauri-apps/plugin-fs')
    const stop = await watchImmediate(notePath, () => {
      // 大纲编辑器挂着同一份笔记时,由它负责重载(重载后会顺手调 setAnswersFromText
      // 刷新本索引)。这里再插一脚会走 loadAnswersFor 的「用内存树」分支,反而拿旧树
      // 遮蔽掉盘上的新内容 —— 单一归属,不重叠。
      if (outline.docPath === notePath) return
      // 合并抖动;自己写盘引发的回调也会走到这里,重载是幂等的,无害。
      if (reloadTimer) clearTimeout(reloadTimer)
      reloadTimer = setTimeout(() => { void loadAnswersFor(mainPath) }, 300)
    })
    if (watched === notePath) unwatch = stop
    else { try { stop() } catch { /* 期间已切换文档 */ } }
  } catch {
    // 文件不存在 / 文件系统不支持监听 → 静默降级,切换文档时仍会重新加载
    watched = null
  }
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
    if (outline.docPath === notePath) {
      setAnswersFromText(notePath, serializeDoc(false))
      void watchCompanion(notePath, mainPath)
      return
    }
    const fs = await import('@tauri-apps/plugin-fs')
    if (!(await fs.exists(notePath).catch(() => false))) { clearAnswers(); return }
    const text = await fs.readTextFile(notePath).catch(() => null)
    if (text == null) { clearAnswers(); return }
    setAnswersFromText(notePath, text)
    void watchCompanion(notePath, mainPath)
  } catch (e) {
    console.warn('[answers] load failed:', e)
    clearAnswers()
  }
}
