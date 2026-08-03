// src/lib/outline/backlinks-io.svelte.ts
import { watchImmediate } from '@tauri-apps/plugin-fs'
import { outline, bump } from './store.svelte'
import { buildFolderIndex, refreshFileInIndex, resolveTarget, detectNameCollisions } from './backlinks'
import { sanitizeFileName } from './slug'
import { outlineDirs } from './dirs.svelte'
import { folderView, parentDir } from '../folder-view.svelte'
import { openFile } from '../tabs.svelte'
import { t } from '../i18n/store.svelte'
import { pushToast } from '../toast.svelte'
import { sotvaultStore } from '../sotvault.svelte'
import { isUnder } from '../recent-merge'
import { joinPath } from '../fs'
import { ensureOutlineFile, newPageFileText } from './create'
import { CONCEPT_TYPE } from '../okf/concept'

let unwatch: (() => void) | null = null
let indexedRoot: string | null = null
let indexGen = 0

/** 索引根(spec §5):文件在 vault 内 → vault 根(全局命名空间);
 *  否则维持现状(FolderView 根 → 文件所在目录)。 */
function indexRootFor(path: string): string {
  const vault = sotvaultStore.vaultRoot
  if (vault && isUnder(path, vault)) return vault
  return folderView.rootDir ?? parentDir(path)
}

/** 面板首次显示/主文件换目录时调用；插件关闭时 teardownIndex() */
export async function ensureIndex(mainPath: string): Promise<void> {
  const root = indexRootFor(mainPath)
  if (indexedRoot === root && outline.backlinkIndex) return
  teardownIndex()
  const gen = ++indexGen
  indexedRoot = root
  const idx = await buildFolderIndex(root, [outlineDirs.wikipage, outlineDirs.dailynote], (legacyPath) => {
    pushToast({ level: 'warn', message: t('outline.migrate.conflict', { path: legacyPath }) })
  })
  if (gen !== indexGen) return   // superseded by teardown or a concurrent call
  outline.backlinkIndex = idx
  bump()
  const collisions = detectNameCollisions(idx)
  if (collisions.size > 0) {
    const [name, files] = [...collisions.entries()][0]
    pushToast({ level: 'warn', message: t('outline.nameCollision', {
      n: String(collisions.size), name, files: files.join('\n') }) })
    console.warn('[outline] name collisions:', Object.fromEntries(collisions))
  }
  let timer: ReturnType<typeof setTimeout> | null = null
  const pending = new Set<string>()
  watchImmediate(root, (ev) => {
    for (const p of (ev.paths ?? [])) if (/\.notes?\.md$/i.test(p)) pending.add(p)
    if (timer) clearTimeout(timer)
    timer = setTimeout(async () => {
      const current = outline.backlinkIndex
      if (!current) return
      for (const p of [...pending]) { pending.delete(p); await refreshFileInIndex(current, p) }
      bump()
    }, 300)
  }, { recursive: true })
    .then(s => {
      if (gen !== indexGen) { try { s() } catch { /* ignore */ } }
      else { unwatch = s }
    })
    .catch(e => console.warn('[outline] backlink watch failed:', e))
}

export function teardownIndex(): void {
  indexGen++
  if (unwatch) { try { unwatch() } catch { /* ignore */ } unwatch = null }
  outline.backlinkIndex = null
  indexedRoot = null
}

/** 点击 [[页面]]:全局解析(resolveTarget);未解析 → vault 内建 wikipage
 *  大纲页,vault 外维持旧行为(同目录建 .md)。 */
export async function openPageOrCreate(target: string): Promise<void> {
  // 日期链接规范形式(spec §6):先于索引匹配,按路径规则直达 dailynote
  {
    const { parseDateLink, ensureDailyNote } = await import('./daily')
    if (parseDateLink(target)) {
      try {
        const p = await ensureDailyNote(target)
        if (p) { await openFile(p); return }
        // vault 未配置:落回普通解析/建页逻辑
      } catch (e) {
        console.warn('[outline] open daily note failed:', e)
        pushToast({ level: 'error', message: String(e) })
        return
      }
    }
  }
  const idx = outline.backlinkIndex
  const existing = idx ? (resolveTarget(idx, target) ?? resolveTarget(idx, sanitizeFileName(target))) : null
  if (existing) { await openFile(existing); return }
  const safe = sanitizeFileName(target)
  const docPath = outline.docPath
  const vault = sotvaultStore.vaultRoot
  // in-vault 判定:编辑器挂载的 doc 在 vault 内,或当前索引根就是 vault
  // (面板点击时 docPath 为空——面板不挂载全局 store,靠 indexedRoot 判定)
  const inVault = vault && ((docPath != null && isUnder(docPath, vault)) || indexedRoot === vault)
  if (inVault) {
    // spec §5:vault 内未解析链接 → vault/{wikipage}/{slug}.note.md,fm title 存原文
    try {
      const { mkdir } = await import('@tauri-apps/plugin-fs')
      const dir = joinPath(vault, outlineDirs.wikipage)
      await mkdir(dir, { recursive: true }).catch(() => {})
      const path = joinPath(dir, `${safe}.note.md`)
      await ensureOutlineFile(path, target, CONCEPT_TYPE.wikiPage)
      await openFile(path)
    } catch (e) {
      console.warn('[outline] create wiki page failed:', e)
      pushToast({ level: 'error', message: String(e) })
    }
    return
  }
  // vault 外:维持现状(同目录建 .md)
  const dir = indexedRoot ?? (docPath ? parentDir(docPath) : null)
  if (!dir) return
  const path = joinPath(dir, `${safe}.md`)
  try {
    const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
    if (!(await exists(path).catch(() => false))) {
      await writeTextFile(path, newPageFileText(safe))
    }
    await openFile(path)
  } catch (e) {
    console.warn('[outline] create page failed:', e)
    pushToast({ level: 'error', message: String(e) })
  }
}
