import { ask } from '@tauri-apps/plugin-dialog'
import { folderView } from '../folder-view.svelte'
import { dirname, normalize, relative } from '../paths'
import { sotvaultStore } from '../sotvault.svelte'
import type { Tab } from '../tabs.svelte'
import { decodeJsonCanvas, isKnownCanvasNode } from './index'

function resourceRoot(path: string): string {
  const vaultRoot = sotvaultStore.vaultRoot
  if (vaultRoot && relative(vaultRoot, path) !== null) return normalize(vaultRoot)
  const folderRoot = folderView.rootDir
  if (folderRoot && relative(folderRoot, path) !== null) return normalize(folderRoot)
  return dirname(path)
}

/**
 * JSON Canvas paths are relative to a Vault/root, not automatically portable.
 * A cross-root Save As therefore requires an explicit acknowledgement instead
 * of silently pretending that referenced files were copied.
 */
export async function confirmCanvasSaveAsReferences(
  tab: Pick<Tab, 'filePath' | 'currentContent'>,
  newPath: string,
): Promise<boolean> {
  if (!tab.filePath || resourceRoot(tab.filePath) === resourceRoot(newPath)) return true
  const decoded = decodeJsonCanvas(tab.currentContent)
  if (!decoded.ok) return true
  const referenceCount = decoded.document.nodes.reduce((count, entry) => {
    if (!isKnownCanvasNode(entry)) return count
    if (entry.type === 'file' && entry.file) return count + 1
    if (entry.type === 'group' && entry.background) return count + 1
    return count
  }, 0)
  if (referenceCount === 0) return true
  return ask(
    `该画布包含 ${referenceCount} 个文件或背景引用。跨目录/工作区另存不会复制这些依赖，副本中可能出现断链。仍要保留原引用并继续吗？`,
    {
      title: '另存画布',
      kind: 'warning',
      okLabel: '保留引用并保存',
      cancelLabel: '取消',
    },
  )
}
