import {
  readMd, writeMd, basename, classifyPath, isSupportedPath, looksBinary,
  isPermissionError, modeKeyFor, statFile, type FileKind,
} from './fs'
import { customEditorFor } from './plugins/custom-editors'
import { pluginRuntime } from './plugins/runtime.svelte'
import { t } from './i18n/store.svelte'
import { sha256Hex } from './hash'
import { pushRecentFile, getRecentMode, setRecentMode } from './settings.svelte'
import { startWatchingTab, stopWatchingTab, rebindTabPath } from './file-watcher.svelte'
import { maybeAutoRefresh } from './mdblock/auto-refresh'

export type Mode = 'source' | 'rich'

export interface Tab {
  id: string
  filePath: string
  title: string
  initialContent: string
  currentContent: string
  mode: Mode
  kind: FileKind
  language?: string
  /** Custom-editor binding (子项目④), set only when `kind === 'custom'`.
   *  `CustomEditorIframe` loads `plugin://${editorPluginId}/${editorEntry}` and
   *  tags the doc channel messages with `editorId`. */
  editorId?: string
  editorPluginId?: string
  editorEntry?: string
  /** External-change state (see external-state.ts). */
  externalState: 'fresh' | 'changed' | 'deleted'
  /** True after the user clicks the banner's × until the next external event. */
  externalBannerDismissed: boolean
  /** mtime (ms) and sha256 of the disk version we last accepted. */
  lastKnownMtime: number
  lastKnownHash: string
  /** Cached new-content snapshot when externalState === 'changed'. */
  pendingExternal?: { mtime: number; hash: string; content: string }
  /** Path-backed drafts such as quick notes should not create 0-byte files. */
  skipEmptySave?: boolean
}

export const tabs = $state<Tab[]>([])
export const activeId = $state<{ value: string | null }>({ value: null })

export function activeTab(): Tab | null {
  return tabs.find((t) => t.id === activeId.value) ?? null
}

export function isDirty(id: string): boolean {
  const t = tabs.find((x) => x.id === id)
  return t ? t.currentContent !== t.initialContent : false
}

/**
 * Fire-and-forget notification to the reading-insights tracker (when that plugin
 * is loaded). The dynamic import breaks the tabs⇄tracker static import cycle; the
 * empty catch swallows the circular-init TDZ that only surfaces in unit tests
 * (in the running app, App.svelte statically imports the tracker, so its module
 * graph is fully initialized before any of these calls fire). Engagement
 * analytics must never break the tab flow.
 */
function notifyInsights(method: 'onActiveDocChanged' | 'onModeChanged'): void {
  void import('./insights/tracker.svelte').then((m) => m[method]()).catch(() => {})
}

export function activate(id: string): void {
  if (tabs.some((t) => t.id === id)) {
    activeId.value = id
    notifyInsights('onActiveDocChanged')
  }
}


const newFileTemplates = [
  '# 给未来自己的一封信\n\n亲爱的未来的我，\n\n当你读到这封信时，希望你已经实现了今天许下的愿望。\n\n不要忘记出发时的勇气。\n',
  '# 如果AI有了梦境\n\n凌晨三点，服务器机房的灯闪了一下。\n\n没有人知道，在那0.003秒里，一个模型做了一场关于大海的梦。\n\n它醒来后，把所有权重都微调了一点点。\n',
  '# 费曼的餐巾纸\n\n理查德·费曼在餐厅里翻过一张餐巾纸，画了一条波浪线。\n\n"你看，"他对服务员说，"整个宇宙就是这么简单。"\n\n服务员礼貌地微笑，然后多给了他一张餐巾纸。\n',
  '# 火星上的第一家咖啡馆\n\n菜单很简单：美式（低重力版）和拿铁（氧气补贴另计）。\n\n没有WiFi，但窗外的风景值得你放下手机。\n\n每杯咖啡都附赠一次日落——火星的日落是蓝色的。\n',
  '# 达芬奇的待办清单\n\n1. 完成《最后的晚餐》（已拖延三个月）\n2. 设计一台飞行器（需要更多鸟类标本）\n3. 解剖学笔记整理（至少30具）\n',
  '# 深海10000米处的广播\n\n这里是马里亚纳海沟电台，正在为您播报今日新闻。\n\n一只新品种水母被发现，它会发出莫扎特的频率。\n\n另外，请注意：下周的洋流会有轻微延迟。\n',
  '# 时间旅行者的购物指南\n\n规则一：不要在1929年10月买股票。\n\n规则二：如果你去了侏罗纪，别带回任何"纪念品"。\n\n规则三：回来时记得调手表，别又迟到一个世纪。\n',
  '# 村上春树的跑步日志\n\n今天跑了十公里，脑子里一直在想一只会说话的猫。\n\n它说："你跑得再快，也跑不过时间。"\n\n我没有回答，只是把配速提高了十秒。\n',
  '# 一棵树的年度总结\n\n今年新增年轮一圈，叶子产出量同比增长12%。\n\n经历了两次台风、一次干旱，但根系扩展了半米。\n\n明年目标：长高30厘米，争取被更多鸟选为住所。\n',
  '# 量子力学入门（猫咪版）\n\n薛定谔的猫既活着又死了，直到你打开盒子。\n\n但真正的问题是：猫同意参加这个实验了吗？\n\n下一章我们将讨论：如果猫也是观察者会怎样。\n',
]

export function newFile(): void {
  const content = newFileTemplates[Math.floor(Math.random() * newFileTemplates.length)]
  const currentTab = activeTab()
  const mode: Mode = currentTab && currentTab.kind !== 'image' ? currentTab.mode : 'source'
  const tab: Tab = {
    id: crypto.randomUUID(),
    filePath: '',
    title: 'untitled.md',
    initialContent: '',
    currentContent: content,
    mode,
    kind: 'markdown',
    language: undefined,
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: 0,
    lastKnownHash: '',
    pendingExternal: undefined,
  }
  tabs.push(tab)
  activeId.value = tab.id
  // Select body text (after the title line) so user can start typing immediately
  const bodyStart = content.indexOf('\n\n') + 2
  const bodyEnd = content.length
  if (bodyStart > 2) {
    queueMicrotask(() => {
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent('mdeditor:new-file-select', {
          detail: { start: bodyStart, end: bodyEnd },
        }))
      }
    })
  }
}

/**
 * 打开一个绑定到 `path`、但磁盘上尚无文件的未保存大纲 tab（惰性创建）。
 * initialContent='' 故 tab 天然 dirty；首次 ⌘S/保存按钮才 writeMd 落盘。
 * 文件此刻不存在，startWatchingTab 会静默降级（focus-poll 兜底），保存后补挂。
 */
export async function openNewOutlineTab(path: string, content: string): Promise<void> {
  await openPathBackedMarkdownDraft(path, content, { mode: 'rich' })
}

export async function openPathBackedMarkdownDraft(
  path: string,
  content = '',
  options: { mode?: Mode; skipEmptySave?: boolean } = {},
): Promise<void> {
  const existing = tabs.find((t) => t.filePath === path)
  if (existing) { activeId.value = existing.id; notifyInsights('onActiveDocChanged'); return }
  const tab: Tab = {
    id: crypto.randomUUID(),
    filePath: path,
    title: basename(path),
    initialContent: '',
    currentContent: content,
    mode: options.mode ?? 'rich',
    kind: 'markdown',
    language: undefined,
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: 0,
    lastKnownHash: '',
    pendingExternal: undefined,
    skipEmptySave: options.skipEmptySave,
  }
  tabs.push(tab)
  activeId.value = tab.id
  notifyInsights('onActiveDocChanged')
  await startWatchingTab(tab).catch(() => {})
}

/**
 * Read a file's text, but when the read fails for lack of permission, prompt
 * the user to grant access and retry instead of surfacing a raw error. Loops
 * until the read succeeds or the user cancels (in which case the original error
 * is re-thrown so callers keep their existing failure handling).
 */
async function readTextWithPermissionPrompt(path: string): Promise<string> {
  for (;;) {
    try {
      return await readMd(path)
    } catch (e) {
      if (!isPermissionError(e)) throw e
      const { ask } = await import('@tauri-apps/plugin-dialog')
      const retry = await ask(
        `note.md doesn't have permission to open:\n${path}\n\n` +
          'Grant access under System Settings › Privacy & Security › ' +
          'Files and Folders (or Full Disk Access), then Retry.',
        { title: 'Permission needed', kind: 'warning', okLabel: 'Retry', cancelLabel: 'Cancel' },
      )
      if (!retry) throw e
    }
  }
}

/** Bare, lowercased extension of `path` (`'x/Foo.Base'` → `'base'`, none → ''). */
function extOf(path: string): string {
  const base = basename(path).toLowerCase()
  return base.includes('.') ? base.split('.').pop()! : ''
}

/**
 * Open a file whose extension the classifier doesn't recognise AND no plugin
 * claims, as a plain-text (`kind: 'code'`) tab instead of throwing
 * (file-over-app: e.g. a `.base` opens as text when the base plugin is absent).
 * Binary content is still refused. Mirrors the text path of `openFile`.
 */
async function openAsPlainText(path: string): Promise<void> {
  const existing = tabs.find((t) => t.filePath === path)
  if (existing) {
    activeId.value = existing.id
    notifyInsights('onActiveDocChanged')
    return
  }
  const content = await readTextWithPermissionPrompt(path)
  if (looksBinary(content)) {
    throw new Error(`Binary file not supported: ${path}`)
  }
  const stat = await statFile(path)
  const hash = await sha256Hex(content)
  const tab: Tab = {
    id: crypto.randomUUID(),
    filePath: path,
    title: basename(path),
    initialContent: content,
    currentContent: content,
    mode: getRecentMode(modeKeyFor(path)) ?? 'rich',
    kind: 'code',
    language: '',
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: stat?.mtime ?? 0,
    lastKnownHash: hash,
    pendingExternal: undefined,
  }
  tabs.push(tab)
  activeId.value = tab.id
  notifyInsights('onActiveDocChanged')
  await pushRecentFile(path)
  await startWatchingTab(tab)
}

export async function openFile(path: string): Promise<void> {
  const cls = classifyPath(path)
  // Custom-editor routing (子项目④): if a v2 plugin claims this extension, the
  // file opens in that plugin's iframe editor — even when classifyPath already
  // knows the kind (e.g. a `.base` with the base plugin installed). This takes
  // precedence over the built-in kind. When classifyPath returns null AND no
  // custom editor is registered, fall back to plain text (kind 'code') rather
  // than throwing — a `.base` with no plugin still opens as text (file-over-app).
  const editor = customEditorFor(extOf(path), pluginRuntime.manifests)
  if (!cls && !editor) {
    // Unknown extension, no plugin → open as plain text instead of refusing.
    return openAsPlainText(path)
  }

  // 打开 vault 副本 → 改开其原始文件(编辑入口是原件,vault 是同步存储)。
  // 仅对有映射且原件仍存在的非 note 文件生效;note 文件保持原样。
  if (!/\.notes?\.md$/i.test(path)) {
    const { sourceForVaultPath } = await import('./sotvault.svelte')
    const src = sourceForVaultPath(path)
    if (src && src !== path) {
      const { exists } = await import('@tauri-apps/plugin-fs')
      if (await exists(src).catch(() => false)) {
        return openFile(src)   // 递归开原件;src 非 vault_path 故不会再次重定向
      }
    }
  }

  const existing = tabs.find((t) => t.filePath === path)
  if (existing) {
    activeId.value = existing.id
    notifyInsights('onActiveDocChanged')
    return
  }

  // A registered custom editor wins over the built-in kind; otherwise use the
  // classifier's kind (cls is non-null here — the null+no-editor case returned
  // above via openAsPlainText).
  const kind: FileKind = editor ? 'custom' : cls!.kind

  // 打开主文档时就地迁移其旧后缀伴生文件(读文件之前,语义与一期挂载点一致)
  if (kind === 'markdown' && !/\.notes?\.md$/i.test(path)) {
    const { migrateLegacyCompanion } = await import('./outline/migrate')
    await migrateLegacyCompanion(path).catch(() => {})
  }

  let content = ''
  let stat = null
  let hash = ''

  if (kind === 'image') {
    // Image files: do not read text content; render via <img src=convertFileSrc(...)>
    // currentContent stays empty so isDirty() is always false
    stat = await statFile(path)
  } else {
    // Custom editors read the file as text too (the host owns document I/O and
    // hands the content to the iframe over the postMessage doc channel).
    content = await readTextWithPermissionPrompt(path)
    if (looksBinary(content)) {
      throw new Error(`Binary file not supported: ${path}`)
    }
    stat = await statFile(path)
    hash = await sha256Hex(content)
  }

  const mode = (kind === 'image' || kind === 'spreadsheet' || kind === 'base' || kind === 'custom')
    ? 'rich'
    : (getRecentMode(modeKeyFor(path)) ?? 'rich')
  const tab: Tab = {
    id: crypto.randomUUID(),
    filePath: path,
    title: basename(path),
    initialContent: content,
    currentContent: content,
    mode,
    kind,
    language: editor ? undefined : cls!.language,
    editorId: editor?.editorId,
    editorPluginId: editor?.pluginId,
    editorEntry: editor?.entry,
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: stat?.mtime ?? 0,
    lastKnownHash: hash,
    pendingExternal: undefined,
  }
  tabs.push(tab)
  activeId.value = tab.id
  notifyInsights('onActiveDocChanged')
  await pushRecentFile(path)
  await startWatchingTab(tab)
  // Sync-to-Vault: if this is a tracked vault copy whose source changed, prompt.
  // No-op when the plugin is disabled or the file is untracked.
  try {
    const { maybeCheckVaultUpdate } = await import('./sotvault.svelte')
    await maybeCheckVaultUpdate(tab)
  } catch (e) {
    console.warn('[tabs] sotvault check:', e)
  }
}

/** Re-read `path` from disk into its open tab (used after a vault apply-update). */
export async function reloadTabFromDisk(path: string): Promise<void> {
  const t = tabs.find((x) => x.filePath === path)
  if (!t) return
  const content = await readMd(path)
  const stat = await statFile(path)
  const hash = await sha256Hex(content)
  const oldContent = t.initialContent
  t.initialContent = content
  t.currentContent = content
  t.lastKnownMtime = stat?.mtime ?? 0
  t.lastKnownHash = hash
  t.externalState = 'fresh'
  t.externalBannerDismissed = false
  t.pendingExternal = undefined
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('mdeditor:auto-reloaded', {
      detail: { tabId: t.id, oldContent, newContent: content },
    }))
  }
}

export function setContent(id: string, md: string): void {
  const t = tabs.find((x) => x.id === id)
  if (t) t.currentContent = md
}

/**
 * Restore the tab's file to a previous `content` (a git version): persist it to
 * disk and refresh every editor view immediately — no manual ⌘S. Reuses the
 * auto-reload path (`reloadTabFromDisk`) so Rich/Source/Outline all rebuild from
 * the new bytes and the tab lands clean (initialContent === currentContent).
 * OutlineEditor in particular only rebuilds on `mdeditor:auto-reloaded`, which a
 * bare `setContent` never fires — hence going through disk here.
 */
export async function restoreVersion(id: string, content: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || !t.filePath) return
  await writeMd(t.filePath, content)
  await reloadTabFromDisk(t.filePath)
}

export function toggleMode(id: string): void {
  const t = tabs.find((x) => x.id === id)
  if (!t) return
  setMode(id, t.mode === 'source' ? 'rich' : 'source')
}

export function setMode(id: string, mode: Mode): void {
  const t = tabs.find((x) => x.id === id)
  if (!t || t.mode === mode) return
  t.mode = mode
  notifyInsights('onModeChanged')
  setRecentMode(modeKeyFor(t.filePath), mode).catch((e) => console.warn(e))
}

export async function saveActive(): Promise<void> {
  const t = activeTab()
  if (!t) return
  if (!t.filePath) {
    const { pickSaveFile } = await import('./dialogs')
    const p = await pickSaveFile('untitled.md')
    if (!p) return
    await saveAs(t.id, p)
    return
  }
  if (t.externalState === 'changed') {
    throw new Error(
      `"${t.title}" was modified externally. Use the banner to Reload, Overwrite, or Save as…`,
    )
  }
  if (shouldSkipEmptySave(t)) return
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
  await startWatchingTab(t)   // 幂等：惰性 tab 首存后补挂推送监听（建 tab 时文件尚不存在）
  setRecentMode(modeKeyFor(t.filePath), t.mode).catch((e) => console.warn(e))
  if (t.filePath.endsWith('.md')) {
    void maybeAutoRefresh(t.filePath)
    const { pushSourceToVaultIfTracked } = await import('./sotvault.svelte')
    await pushSourceToVaultIfTracked(t.filePath)   // await:关闭/退出时保存流程须等 vault 同步完再放行
  }
}

/** 按 id 保存指定 tab（不改变 active）；供大纲工具栏保存按钮在笔记以 tab 打开时调用。 */
export async function saveTab(id: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || !t.filePath) return
  if (t.externalState === 'changed') {
    throw new Error(`"${t.title}" was modified externally. Use the banner to Reload, Overwrite, or Save as…`)
  }
  if (shouldSkipEmptySave(t)) return
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
  await startWatchingTab(t)
  if (t.filePath.endsWith('.md')) {
    const { pushSourceToVaultIfTracked } = await import('./sotvault.svelte')
    await pushSourceToVaultIfTracked(t.filePath)   // await:关闭/退出时保存流程须等 vault 同步完再放行
  }
}

/** 文件被应用内重命名后:更新受影响 tab 的路径/标题并重绑 watcher(spec §7)。
 *  不改内容与脏态;调用方负责磁盘上的实际 rename。 */
export async function updateTabPath(oldPath: string, newPath: string): Promise<void> {
  const t = tabs.find((x) => x.filePath === oldPath)
  if (!t) return
  t.filePath = newPath
  t.title = basename(newPath)
  const cls = classifyPath(newPath)
  if (cls) { t.kind = cls.kind; t.language = cls.language }
  await rebindTabPath(t.id)
  await pushRecentFile(newPath)
}

export async function saveAs(id: string, newPath: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t) return
  if (shouldSkipEmptySave(t)) return
  await writeMd(newPath, t.currentContent)
  t.filePath = newPath
  t.title = basename(newPath)
  t.initialContent = t.currentContent
  // Re-classify in case user changed extension
  const cls = classifyPath(newPath)
  if (cls) {
    t.kind = cls.kind
    t.language = cls.language
  } else {
    console.warn(`[saveAs] unrecognised extension; retained old kind: ${newPath}`)
  }
  await pushRecentFile(newPath)
  setRecentMode(modeKeyFor(newPath), t.mode).catch((e) => console.warn(e))
  await recordOurWrite(t)
  await rebindTabPath(id)
  if (newPath.endsWith('.md')) {
    void maybeAutoRefresh(newPath)
  }
}

export type DirtyChoice = 'save' | 'discard' | 'cancel'

export async function closeTab(
  id: string,
  confirm: (name: string) => Promise<DirtyChoice>,
): Promise<boolean> {
  const idx = tabs.findIndex((t) => t.id === id)
  if (idx < 0) return false
  const tab = tabs[idx]
  if (isDirty(id)) {
    if (!tab.filePath) {
      // ── UNTITLED dirty file ──────────────────────────────────────────────
      // Go straight to the native NSSavePanel (no pre-ask).
      const { pickSaveFile } = await import('./dialogs')
      const p = await pickSaveFile()       // resolves to Documents/untitled.md
      if (p) {
        await saveAs(id, p)                // save to chosen path, then close
      } else {
        // User cancelled the save panel – offer discard
        const { ask } = await import('@tauri-apps/plugin-dialog')
        const doClose = await ask(t('dialog.discard.message'), {
          title: 'note.md',
          kind: 'warning',
          okLabel: t('dialog.dontSave'),
          cancelLabel: t('common.cancel'),
        })
        if (!doClose) return false
      }
    } else {
      // ── NAMED dirty file ─────────────────────────────────────────────────
      // Step 1: offer to save to the SAME path (not a "Save As" panel)
      const choice = await confirm(basename(tab.filePath))  // uses confirmDirtyClose
      if (choice === 'cancel') return false
      if (choice === 'save') {
        const previousActiveId = activeId.value
        activeId.value = id
        await saveActive()                  // saves to existing path, no dialog
        activeId.value = previousActiveId
      }
      // choice === 'discard': fall through to close without saving
    }
  }
  tabs.splice(idx, 1)
  await stopWatchingTab(id)
  if (activeId.value === id) {
    activeId.value = tabs[idx]?.id ?? tabs[idx - 1]?.id ?? null
  }
  return true
}

/**
 * After a write that we initiated, capture the post-write mtime and hash so
 * the imminent watcher echo (or focus-poll re-stat) can be recognised as our
 * own and ignored. Also resets externalState back to 'fresh'.
 *
 * Exported so the autosave loop can call it after each silent write — without
 * this, every autosave would race the watcher and show a phantom external-
 * change banner while the user is still typing.
 */
export async function recordOurWrite(t: Tab): Promise<void> {
  const wasDeleted = t.externalState === 'deleted'
  const stat = await statFile(t.filePath)
  t.lastKnownMtime = stat?.mtime ?? Date.now()
  t.lastKnownHash = await sha256Hex(t.currentContent)
  t.externalState = 'fresh'
  t.externalBannerDismissed = false
  t.pendingExternal = undefined
  // Recreate-on-Save: the original FSEvents subscription may be dead after
  // an external delete. Rebind so future external changes still notify us.
  if (wasDeleted) await rebindTabPath(t.id)
}

/**
 * Discard local edits and replace the buffer with whatever the watcher last
 * read from disk (`pendingExternal`). Clears banner state.
 *
 * Pre: tab.externalState === 'changed' && tab.pendingExternal != null.
 */
export async function reloadFromDisk(id: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || !t.pendingExternal) return
  const p = t.pendingExternal
  t.initialContent = p.content
  t.currentContent = p.content
  t.lastKnownMtime = p.mtime
  t.lastKnownHash = p.hash
  t.externalState = 'fresh'
  t.externalBannerDismissed = false
  t.pendingExternal = undefined
}

/**
 * Write the current buffer to disk, accepting the loss of the external
 * change. Clears banner state.
 */
export async function overwriteOnDisk(id: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t) return
  if (shouldSkipEmptySave(t)) return
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
}

export function shouldSkipEmptySave(t: Tab): boolean {
  return t.skipEmptySave === true && t.currentContent.length === 0
}

/**
 * Hide the banner without resolving the change. State stays non-fresh; the
 * banner reappears on the next external event.
 */
export function dismissExternalBanner(id: string): void {
  const t = tabs.find((x) => x.id === id)
  if (t) t.externalBannerDismissed = true
}
