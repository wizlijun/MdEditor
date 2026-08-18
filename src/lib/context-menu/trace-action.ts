// 右键「溯源」→ 打开 idea-spark 并预填 /溯源 委托文本。
// 选区文本即锚(与 answer-sites 的「按批注文本锚定」同一约定);agent 侧协议
// 见 idea-spark 播种的 trace-source 任务模板。
import { invoke } from '@tauri-apps/api/core'
import { activeTab } from '../tabs.svelte'
import { sotvaultStore } from '../sotvault.svelte'

/** 预填不是产物,只是委托底稿——超长选区截断即可,截断处如实标注。 */
const MAX_SELECTION = 8000

export function buildTraceSeed(selection: string, docPath: string, vaultRoot: string | null): string {
  let sel = selection.trim()
  if (sel.length > MAX_SELECTION) sel = `${sel.slice(0, MAX_SELECTION)}\n…(选区过长已截断)`
  const quoted = sel.split('\n').map((l) => `> ${l}`).join('\n')
  const root = vaultRoot?.replace(/\/+$/, '')
  const rel = root && docPath.startsWith(`${root}/`) ? docPath.slice(root.length + 1) : docPath
  const source = docPath ? `\n\n源文档: ${rel}\n` : '\n'
  return `/溯源 \n\n${quoted}${source}`
}

export async function openTraceDelegation(selection: string): Promise<void> {
  const text = buildTraceSeed(selection, activeTab()?.filePath ?? '', sotvaultStore.vaultRoot)
  try {
    await invoke('plugin_v2_open_window', {
      pluginId: 'notemd.idea-spark',
      windowId: 'main',
      seed: { text },
    })
  } catch (e) {
    console.error('[trace] 打开 idea-spark 失败(插件未安装?):', e)
  }
}
