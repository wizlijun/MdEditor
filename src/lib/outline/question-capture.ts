// src/lib/outline/question-capture.ts
// Headless 提问捕获:大纲编辑器未挂载时,主文档里出现「提问批注」({>>…?<<})
// 也要把伴生 .note.md 写上盘——外部 agent 只能读到磁盘文件(spec §2)。
// 挂在 tabs.setContent 上,廉价门卫 + 按路径防抖;大纲已挂载同文档时让位
// (它的派生管线 + arm 会落盘,双写会互踩)。
import { deriveAutoItems } from './derive'
import { syncAutoItems } from './sync'
import { parseOutline, serializeOutline } from './markdown'
import { treeHasQuestion, isQuestionText } from './model'
import { outline, companionPathFor, noteTextHasContent } from './store.svelte'
import { planNoteHome } from './note-home'

/** 主文档文本里是否存在「提问批注」。廉价门卫,避免每次输入都走重逻辑 */
export function mdHasQuestionAnnotation(md: string): boolean {
  for (const m of md.matchAll(/\{>>(.*?)<<\}/g)) if (isQuestionText(m[1])) return true
  return false
}

const timers = new Map<string, ReturnType<typeof setTimeout>>()

export function scheduleQuestionCapture(mainPath: string | null | undefined, md: string): void {
  if (!mainPath || !/\.md$/i.test(mainPath) || /\.notes?\.md$/i.test(mainPath)) return
  // 先取消在途防抖:1.5s 内「输入?又删掉」时,陈旧捕获不得照写
  const prev = timers.get(mainPath)
  if (prev) { clearTimeout(prev); timers.delete(mainPath) }
  if (!mdHasQuestionAnnotation(md)) return
  timers.set(mainPath, setTimeout(() => {
    timers.delete(mainPath)
    void captureQuestions(mainPath, md)
  }, 1500))
}

async function captureQuestions(mainPath: string, md: string): Promise<void> {
  try {
    // 大纲编辑器已挂载同一文档 → 让位给它的 intent-save 管线
    const companion = companionPathFor(mainPath)
    if (companion && outline.docPath === companion) return
    const { sotvaultStore, syncSourceToVaultAsHome, refreshSotvault } = await import('../sotvault.svelte')
    const fs = await import('@tauri-apps/plugin-fs')
    const legacyNoteExists = companion ? await fs.exists(companion).catch(() => false) : false
    const plan = planNoteHome(mainPath, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
      legacyNoteExists,
    })
    let notePath: string | null = null
    if (plan.action === 'use') notePath = plan.notePath
    else if (plan.action === 'sync') {
      // 首写:源复制进 vault,笔记落 vault 副本旁(伴生笔记只住 vault)
      const rec = await syncSourceToVaultAsHome(mainPath)
      notePath = companionPathFor(rec.vault_path)
      await refreshSotvault()
    } else {
      return   // configure-vault:无 vault 静默跳过;批注仍在 md,配好 vault 后重开即捕获
    }
    if (!notePath) return
    if (outline.docPath === notePath) return   // 建家期间大纲挂上来了(竞态)→ 让位
    const existed = await fs.exists(notePath).catch(() => false)
    const diskText = existed ? await fs.readTextFile(notePath).catch(() => null) : ''
    if (diskText == null) return
    const tree = parseOutline(diskText)
    syncAutoItems(tree, deriveAutoItems(md))
    if (!treeHasQuestion(tree)) return         // 门卫误报(如 ? 在代码块里):不写
    const out = serializeOutline(tree)
    if (out === diskText) return
    // 数据丢失防线:绝不用"无内容"的序列化盖有内容的落点
    if (!noteTextHasContent(out) && noteTextHasContent(diskText)) return
    await fs.writeTextFile(notePath, out)
  } catch (e) {
    console.warn('[question-capture] failed:', e)
  }
}
