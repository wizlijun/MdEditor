// src/lib/outline/question-capture.ts
// Headless 提问捕获:大纲编辑器未挂载时,主文档里出现「提问批注」({>>…?<<})
// 也要把伴生 .note.md 写上盘——外部 agent 只能读到磁盘文件(spec §2)。
// 挂在 tabs.setContent 上,廉价门卫 + 按路径防抖;大纲已挂载同文档时让位
// (它的派生管线 + arm 会落盘,双写会互踩)。
import { deriveAutoItems, type AutoItem } from './derive'
import { syncAutoItems } from './sync'
import { parseOutline, serializeOutline } from './markdown'
import { treeHasQuestion, isQuestionText } from './model'
import { outline, companionPathFor, noteTextHasContent } from './store.svelte'
import { planNoteHome } from './note-home'

/** 只有问号、没有实质内容的批注不算问题——「提问」菜单预填的 `{>>?<<}` 在用户
 *  打字之前就长这样,防抖一到就落盘会往 .note.md 里塞一条空问题。
 *  只把这道守卫加在落盘链路上:isQuestionText 的大纲升格语义保持不变。 */
function isSubstantiveQuestion(s: string): boolean {
  return isQuestionText(s) && s.replace(/[?？\s]/g, '') !== ''
}

/** 主文档文本里是否存在「提问批注」。廉价门卫,避免每次输入都走重逻辑。
 *  正则不识别代码围栏,可能对围栏内的 `{>>?<<}` 误报——真问题由 mdDerivesQuestion 二次确认。 */
export function mdHasQuestionAnnotation(md: string): boolean {
  for (const m of md.matchAll(/\{>>(.*?)<<\}/g)) if (isSubstantiveQuestion(m[1])) return true
  return false
}

/** 派生条目里含提问批注的谓词(deriveAutoItems 已跳过代码围栏,故据此判定为准) */
const isQuestionAnnotation = (it: AutoItem): boolean =>
  it.source === 'annotation' && isSubstantiveQuestion(it.note ?? '')

/** 主文档派生后确有「提问批注」。用于在触碰 vault(复制源文件)之前排除代码块内的假阳性。 */
export function mdDerivesQuestion(md: string): boolean {
  return deriveAutoItems(md).some(isQuestionAnnotation)
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
    // 真问题确认(排除代码块内假阳性)——必须在触碰 vault / 复制源文件之前。
    // 否则围栏里的 `{>>?<<}` 会让廉价门卫放行、把源文件白白同步进 vault 却不写任何笔记。
    const items = deriveAutoItems(md)
    if (!items.some(isQuestionAnnotation)) return
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
    syncAutoItems(tree, items)
    if (!treeHasQuestion(tree)) return         // 兜底:派生后仍无 question(如全被 blocklist 拦)则不写
    const out = serializeOutline(tree)
    if (out === diskText) return
    // 数据丢失防线:绝不用"无内容"的序列化盖有内容的落点
    if (!noteTextHasContent(out) && noteTextHasContent(diskText)) return
    await fs.writeTextFile(notePath, out)
  } catch (e) {
    console.warn('[question-capture] failed:', e)
  }
}
