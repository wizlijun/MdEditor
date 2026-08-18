// 右键「溯源」→ 打开 trace-source 插件窗口并预填委托文本。
// 选区文本即锚(与 answer-sites 的「按批注文本锚定」同一约定);agent 侧协议
// 见 trace-source 插件播种的任务模板(plugins-src/trace-source)。
//
// 协议键语言中立(`Source-Doc:`),与模板 CLAUDE.md 及插件的 `Output:` 行同一
// 套约定——预填在英文/日文界面里不该冒出中文字段名。
import { invoke } from '@tauri-apps/api/core'
import { activeTab } from '../tabs.svelte'
import { sotvaultStore } from '../sotvault.svelte'

/** 右键「溯源」项的宿主开关:装了这个插件才出现(menu-model.ts traceInstalled)。 */
export const TRACE_PLUGIN_ID = 'notemd.trace-source'

/** 预填不是产物,只是委托底稿——超长选区截断即可,截断处如实标注(语言中立)。 */
const MAX_SELECTION = 8000

export function buildTraceSeed(selection: string, docPath: string, vaultRoot: string | null): string {
  let sel = selection.trim()
  if (sel.length > MAX_SELECTION) sel = `${sel.slice(0, MAX_SELECTION)}\n…(selection truncated)`
  const quoted = sel.split('\n').map((l) => `> ${l}`).join('\n')
  const root = vaultRoot?.replace(/\/+$/, '')
  const rel = root && docPath.startsWith(`${root}/`) ? docPath.slice(root.length + 1) : docPath
  const source = docPath ? `\n\nSource-Doc: ${rel}\n` : '\n'
  return `${quoted}${source}`
}

export async function openTraceDelegation(selection: string): Promise<void> {
  const text = buildTraceSeed(selection, activeTab()?.filePath ?? '', sotvaultStore.vaultRoot)
  try {
    await invoke('plugin_v2_open_window', {
      pluginId: TRACE_PLUGIN_ID,
      windowId: 'main',
      seed: { text },
    })
  } catch (e) {
    console.error('[trace] 打开 trace-source 失败(插件未安装?):', e)
  }
}
