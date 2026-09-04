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
import { quickNoteRenameTarget } from './quick-note-name'
import { newFileText } from './new-file'
import { isConfiguredMemoryProjectionPath } from './memory-projection'
import { humanActorNow } from './okf/identity'
import type { CanvasDiskRevision, CanvasSaveResult } from './canvas/io'

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
  /** Exact disk identity used by the Canvas atomic compare-and-replace path. */
  canvasRevision?: CanvasDiskRevision
}

export const tabs = $state<Tab[]>([])
export const activeId = $state<{ value: string | null }>({ value: null })

/** Smallest valid JSON Canvas 1.0 document, used only for first creation. */
export const EMPTY_CANVAS_CONTENT = '{\n  "nodes": [],\n  "edges": []\n}\n'

export function activeTab(): Tab | null {
  return tabs.find((t) => t.id === activeId.value) ?? null
}

function flushMountedDocument(tabId: string): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent('notemd:flush-doc', { detail: { tabId } }))
}

export function isDirty(id: string): boolean {
  const t = tabs.find((x) => x.id === id)
  return t ? t.currentContent !== t.initialContent : false
}

/** v2 USER/MEMORY projections are changed only by the Memory workflow. */
export function isManagedMemoryTab(
  tab: Pick<Tab, 'filePath'>,
): boolean {
  return isConfiguredMemoryProjectionPath(tab.filePath)
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
    if (activeId.value && activeId.value !== id) flushMountedDocument(activeId.value)
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
  const by = humanActorNow()
  const content = newFileText(
    newFileTemplates[Math.floor(Math.random() * newFileTemplates.length)],
    by ? { by, at: new Date().toISOString() } : undefined,
  )
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
  activate(tab.id)
  // Select body text (after the title line) so user can start typing immediately
  const bodyStart = content.indexOf('\n\n') + 2
  const bodyEnd = content.length
  if (bodyStart > 2) {
    queueMicrotask(() => {
      if (typeof window !== 'undefined') {
        window.dispatchEvent(new CustomEvent('notemd:new-file-select', {
          detail: { start: bodyStart, end: bodyEnd },
        }))
      }
    })
  }
}

/**
 * Create a named `.canvas` document, then reopen it through the normal file
 * path so watcher/hash/recent-file state is initialized exactly once. Canvas
 * tabs are deliberately never path-less: cancelling the panel creates no tab.
 */
export async function newCanvas(): Promise<void> {
  const { isIOS } = await import('./platform.svelte')
  let path: string | null
  if (await isIOS()) {
    const [{ documentDir }, { exists }, { sotvaultStore }] = await Promise.all([
      import('@tauri-apps/api/path'), import('@tauri-apps/plugin-fs'), import('./sotvault.svelte'),
    ])
    const dir = (sotvaultStore.vaultRoot || await documentDir()).replace(/[\\/]$/, '')
    path = `${dir}/untitled.canvas`
    for (let suffix = 2; await exists(path).catch(() => false); suffix++) {
      if (suffix > 999) throw new Error('Unable to allocate a unique Canvas filename')
      path = `${dir}/untitled-${suffix}.canvas`
    }
  } else {
    const { pickSaveCanvasFile } = await import('./dialogs')
    path = await pickSaveCanvasFile()
  }
  if (!path) return
  const { canvasDocumentCreate } = await import('./canvas/io')
  const created = await canvasDocumentCreate(path, EMPTY_CANVAS_CONTENT)
  await openFile(created.canonicalPath)
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
  if (existing) { activate(existing.id); return }
  const tab: Tab = {
    id: crypto.randomUUID(),
    filePath: path,
    title: basename(path),
    initialContent: '',
    currentContent: content,
    // Same remembered-mode source as openFile, so a lazily-created draft opens
    // in whichever mode the user last chose for this file type.
    mode: options.mode ?? getRecentMode(modeKeyFor(path)) ?? 'rich',
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
  activate(tab.id)
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
    activate(existing.id)
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
  activate(tab.id)
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
  // `.canvas` is a reserved built-in document surface. Keep this runtime
  // guard even when manifest validation also rejects such a plugin claim.
  const editor = cls?.kind === 'canvas'
    ? null
    : customEditorFor(extOf(path), pluginRuntime.manifests)
  if (!cls && !editor) {
    // Unknown extension, no plugin → open as plain text instead of refusing.
    return openAsPlainText(path)
  }

  // 打开 vault 副本 → 改开其原始文件(编辑入口是原件,vault 是同步存储)。
  // 仅对有映射且原件仍存在的非 note 文件生效;note 文件保持原样。
  if (cls?.kind !== 'canvas' && !/\.notes?\.md$/i.test(path)) {
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
    activate(existing.id)
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
  let canvasRevision: CanvasDiskRevision | undefined

  if (kind === 'image') {
    // Image files: do not read text content; render via <img src=convertFileSrc(...)>
    // currentContent stays empty so isDirty() is always false
    stat = await statFile(path)
  } else if (kind === 'canvas') {
    const { canvasDocumentOpen, canvasMtimeMs } = await import('./canvas/io')
    const opened = await canvasDocumentOpen(path)
    path = opened.canonicalPath
    const canonicalExisting = tabs.find((t) => t.filePath === path)
    if (canonicalExisting) {
      activate(canonicalExisting.id)
      return
    }
    content = opened.text
    canvasRevision = opened.revision
    stat = { mtime: canvasMtimeMs(opened.revision), size: opened.revision.size }
    hash = opened.revision.sha256
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

  const mode = (kind === 'image' || kind === 'spreadsheet' || kind === 'base' || kind === 'canvas' || kind === 'custom')
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
    canvasRevision,
  }
  tabs.push(tab)
  activate(tab.id)
  await pushRecentFile(path)
  await startWatchingTab(tab)
  // Sync-to-Vault: if this is a tracked vault copy whose source changed, prompt.
  // No-op when the plugin is disabled or the file is untracked.
  if (kind !== 'canvas') {
    try {
      const { maybeCheckVaultUpdate } = await import('./sotvault.svelte')
      await maybeCheckVaultUpdate(tab)
    } catch (e) {
      console.warn('[tabs] sotvault check:', e)
    }
  }
}

/** Re-read `path` from disk into its open tab (used after a vault apply-update). */
export async function reloadTabFromDisk(path: string): Promise<void> {
  const t = tabs.find((x) => x.filePath === path)
  if (!t) return
  let content: string
  let mtime: number
  let hash: string
  if (t.kind === 'canvas') {
    const { canvasDocumentOpen, canvasMtimeMs } = await import('./canvas/io')
    const opened = await canvasDocumentOpen(path)
    content = opened.text
    mtime = canvasMtimeMs(opened.revision)
    hash = opened.revision.sha256
    t.canvasRevision = opened.revision
    t.filePath = opened.canonicalPath
    t.title = basename(opened.canonicalPath)
  } else {
    content = await readMd(path)
    const stat = await statFile(path)
    mtime = stat?.mtime ?? 0
    hash = await sha256Hex(content)
  }
  const oldContent = t.initialContent
  t.initialContent = content
  t.currentContent = content
  t.lastKnownMtime = mtime
  t.lastKnownHash = hash
  t.externalState = 'fresh'
  t.externalBannerDismissed = false
  t.pendingExternal = undefined
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('notemd:auto-reloaded', {
      detail: { tabId: t.id, oldContent, newContent: content },
    }))
  }
}

let questionCapture: typeof import('./outline/question-capture') | null = null

export function setContent(id: string, md: string): void {
  const t = tabs.find((x) => x.id === id)
  if (!t || isManagedMemoryTab(t)) return
  t.currentContent = md
  if (!t.filePath || t.kind === 'canvas') return
  // 提问捕获:模块加载后无条件调用(schedule 自带在途取消,批注整体删除也能撤回);
  // 首次加载仍由 {>> 门卫触发——加载前不存在在途 timer
  if (questionCapture) {
    questionCapture.scheduleQuestionCapture(t.filePath, md)
  } else if (md.includes('{>>')) {
    void import('./outline/question-capture').then((m) => {
      questionCapture = m
      m.scheduleQuestionCapture(t.filePath, md)
    })
  }
}

/**
 * Restore the tab's file to a previous `content` (a git version): persist it to
 * disk and refresh every editor view immediately — no manual ⌘S. Reuses the
 * auto-reload path (`reloadTabFromDisk`) so Rich/Source/Outline all rebuild from
 * the new bytes and the tab lands clean (initialContent === currentContent).
 * OutlineEditor in particular only rebuilds on `notemd:auto-reloaded`, which a
 * bare `setContent` never fires — hence going through disk here.
 */
export async function restoreVersion(id: string, content: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || !t.filePath || isManagedMemoryTab(t)) return
  if (t.kind === 'canvas') await persistCanvasSnapshot(t, content)
  else await writeMd(t.filePath, content)
  await reloadTabFromDisk(t.filePath)
}

export function toggleMode(id: string): void {
  const t = tabs.find((x) => x.id === id)
  if (!t || t.kind === 'canvas') return
  setMode(id, t.mode === 'source' ? 'rich' : 'source')
}

export function setMode(id: string, mode: Mode): void {
  const t = tabs.find((x) => x.id === id)
  if (!t || t.kind === 'canvas' || t.mode === mode) return
  t.mode = mode
  notifyInsights('onModeChanged')
  setRecentMode(modeKeyFor(t.filePath), mode).catch((e) => console.warn(e))
}

/**
 * A quick note keeps its generated `…-quick.md` name only until it has a title:
 * the first save after an H1 appears renames the file after that title. It
 * renames once — a note already carrying a title-based name is left alone, so
 * later title edits never move the path out from under existing links.
 *
 * Failures are non-fatal: the note stays under its generated name rather than
 * the save appearing to fail.
 *
 * `requireFinishedTitle` is set by auto-save so a heading that is still being
 * typed does not name the file; an explicit save takes the title as it stands.
 */
export async function renameAutoQuickNoteIfTitled(
  t: Tab,
  requireFinishedTitle = false,
): Promise<void> {
  if (!t.filePath) return
  const name = basename(t.filePath)
  const target = quickNoteRenameTarget(name, t.currentContent, requireFinishedTitle)
  if (!target) return

  const dir = t.filePath.slice(0, t.filePath.length - name.length)
  const { rename, exists } = await import('@tauri-apps/plugin-fs')
  // Never clobber a file that is already there — fall back to `-2`, `-3`, …
  let candidate = target
  for (let n = 2; await exists(dir + candidate).catch(() => false); n++) {
    if (n > 99) return
    candidate = target.replace(/\.md$/i, `-${n}.md`)
  }

  const from = t.filePath
  const to = dir + candidate
  try {
    await rename(from, to)
  } catch (e) {
    console.warn('[quick-note] rename failed:', from, '→', to, e)
    return
  }
  await updateTabPath(from, to)
  // Re-baseline under the new path so the rename does not surface as an
  // external change.
  await recordOurWrite(t)
}

export async function saveActive(): Promise<void> {
  const t = activeTab()
  if (!t) return
  flushMountedDocument(t.id)
  if (!t.filePath) {
    const { pickSaveCanvasFile, pickSaveFile } = await import('./dialogs')
    const p = t.kind === 'canvas'
      ? await pickSaveCanvasFile('untitled.canvas')
      : await pickSaveFile('untitled.md')
    if (!p) return
    await saveAs(t.id, p)
    return
  }
  if (isManagedMemoryTab(t)) return
  if (t.externalState === 'changed') {
    throw new Error(
      `"${t.title}" was modified externally. Use the banner to Reload, Overwrite, or Save as…`,
    )
  }
  if (shouldSkipEmptySave(t)) return
  if (t.kind === 'canvas') {
    await persistCanvasSnapshot(t, t.currentContent)
    await startWatchingTab(t)
    return
  }
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
  await startWatchingTab(t)   // 幂等：惰性 tab 首存后补挂推送监听（建 tab 时文件尚不存在）
  await renameAutoQuickNoteIfTitled(t)   // 可能改写 t.filePath，须在 vault 推送前
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
  if (!t || !t.filePath || isManagedMemoryTab(t)) return
  if (activeId.value === id) flushMountedDocument(id)
  if (t.externalState === 'changed') {
    throw new Error(`"${t.title}" was modified externally. Use the banner to Reload, Overwrite, or Save as…`)
  }
  if (shouldSkipEmptySave(t)) return
  if (t.kind === 'canvas') {
    await persistCanvasSnapshot(t, t.currentContent)
    await startWatchingTab(t)
    return
  }
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
  await startWatchingTab(t)
  await renameAutoQuickNoteIfTitled(t)   // 可能改写 t.filePath，须在 vault 推送前
  if (t.filePath.endsWith('.md')) {
    const { pushSourceToVaultIfTracked } = await import('./sotvault.svelte')
    await pushSourceToVaultIfTracked(t.filePath)   // await:关闭/退出时保存流程须等 vault 同步完再放行
  }
}

/** 文件被应用内重命名后：更新 tab 身份，并把已打开 Canvas 的精确引用作为可撤销事务改写。 */
export async function updateTabPath(oldPath: string, newPath: string): Promise<void> {
  const t = tabs.find((x) => x.filePath === oldPath)
  if (t) {
    const wasCanvas = t.kind === 'canvas'
    t.filePath = newPath
    t.title = basename(newPath)
    const cls = classifyPath(newPath)
    if (cls) { t.kind = cls.kind; t.language = cls.language }
    await rebindTabPath(t.id)
    await pushRecentFile(newPath)
    if (wasCanvas) {
      const { copyCanvasViewport } = await import('../components/canvas/canvas-view-state')
      await copyCanvasViewport(oldPath, newPath, true)
    }
  }
  await rewriteOpenCanvasReferences(oldPath, newPath)
}

async function rewriteOpenCanvasReferences(oldPath: string, newPath: string): Promise<void> {
  const canvasTabs = tabs.filter((entry) => entry.kind === 'canvas')
  if (canvasTabs.length === 0) return
  const [paths, canvas, sotvault, folders] = await Promise.all([
    import('./paths'),
    import('./canvas'),
    import('./sotvault.svelte'),
    import('./folder-view.svelte'),
  ])
  const oldTarget = paths.normalize(oldPath)
  const rootFor = (canvasPath: string): string => {
    const vaultRoot = sotvault.sotvaultStore.vaultRoot
    if (vaultRoot && paths.relative(vaultRoot, canvasPath) !== null) return paths.normalize(vaultRoot)
    const folderRoot = folders.folderView.rootDir
    if (folderRoot && paths.relative(folderRoot, canvasPath) !== null) return paths.normalize(folderRoot)
    return paths.dirname(canvasPath)
  }

  for (const canvasTab of canvasTabs) {
    const decoded = canvas.decodeJsonCanvas(canvasTab.currentContent)
    if (!decoded.ok) continue
    const root = rootFor(canvasTab.filePath)
    const rewrite = (raw: string): string | null => {
      if (!raw || raw.includes('\0') || paths.isAbsolute(raw)) return null
      const resolved = paths.joinPath(root, paths.normalize(raw))
      if (paths.normalize(resolved) !== oldTarget) return null
      const next = paths.relative(root, paths.normalize(newPath))
      return next === null ? null : next.replaceAll('\\', '/')
    }
    const next = canvas.cloneCanvasDocument(decoded.document)
    let changed = false
    for (const entry of next.nodes) {
      if (!canvas.isKnownCanvasNode(entry)) continue
      if (entry.type === 'file') {
        const value = rewrite(entry.file)
        if (value !== null && value !== entry.file) { entry.file = value; changed = true }
      } else if (entry.type === 'group' && entry.background) {
        const value = rewrite(entry.background)
        if (value !== null && value !== entry.background) { entry.background = value; changed = true }
      }
    }
    if (!changed) continue
    const encoded = canvas.encodeJsonCanvas(next)
    const session = canvas.acquireCanvasUiSession(canvasTab.id, canvasTab.currentContent)
    session.history.record('更新文件引用', decoded.document, next)
    canvas.markCanvasUiSessionContent(canvasTab.id, encoded)
    canvasTab.currentContent = encoded
  }
}

export async function saveAs(id: string, newPath: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || isManagedMemoryTab(t)) return
  if (activeId.value === id) flushMountedDocument(id)
  if (shouldSkipEmptySave(t)) return
  const targetIsCanvas = /\.canvas$/i.test(newPath)
  if (t.kind === 'canvas' && !targetIsCanvas) {
    throw new Error('Canvas documents must be saved with a .canvas extension')
  }
  if (t.kind !== 'canvas' && targetIsCanvas) {
    throw new Error('Only Canvas documents can be saved with a .canvas extension')
  }
  const content = t.currentContent
  if (t.kind === 'canvas') {
    await enqueueCanvasOperation(t, async (sourceIdentity) => {
      const {
        canvasDocumentCreate, canvasDocumentProbe, canvasDocumentSave, canvasMtimeMs,
      } = await import('./canvas/io')
      const targetPath = newPath
      const probe = await canvasDocumentProbe(targetPath)
      const saved = probe.kind === 'missing'
        ? await canvasDocumentCreate(targetPath, content)
        : await canvasDocumentSave(targetPath, content, probe.revision)

      // A file operation outside this coordinator (for example an external
      // reload) may have rebound the tab while the Save As panel/write was in
      // flight. The copy was still created, but it must never hijack that newer
      // document identity.
      if (!canvasIdentityMatches(t, sourceIdentity)) {
        throw new Error('Canvas changed identity while Save As was in progress; the saved copy was not opened')
      }

      const { copyCanvasViewport } = await import('../components/canvas/canvas-view-state')
      await copyCanvasViewport(sourceIdentity.path, saved.canonicalPath)

      t.filePath = saved.canonicalPath
      t.title = basename(saved.canonicalPath)
      t.canvasRevision = saved.revision
      if (t.currentContent === content) t.initialContent = content
      t.lastKnownMtime = canvasMtimeMs(saved.revision)
      t.lastKnownHash = saved.revision.sha256
      t.externalState = 'fresh'
      t.externalBannerDismissed = false
      t.pendingExternal = undefined
      await pushRecentFile(saved.canonicalPath)
      await rebindTabPath(id)
    })
    return
  }
  await writeMd(newPath, content)
  t.filePath = newPath
  t.title = basename(newPath)
  if (t.currentContent === content) t.initialContent = content
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

/**
 * Write an independent Canvas copy without rebinding the open tab. This is
 * the mobile "Save As" semantic: the document picker exports bytes, while
 * the current tab keeps its path, revision, dirty baseline and UI session.
 */
export async function exportCanvasCopy(id: string, newPath: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || t.kind !== 'canvas' || isManagedMemoryTab(t)) return
  if (!/\.canvas$/i.test(newPath)) {
    throw new Error('Canvas documents must be exported with a .canvas extension')
  }
  const content = t.currentContent
  const targetPath = newPath
  await enqueueCanvasOperation(t, async (sourceIdentity) => {
    const { canvasDocumentCreate, canvasDocumentProbe, canvasDocumentSave } = await import('./canvas/io')
    const probe = await canvasDocumentProbe(targetPath)
    if (probe.canonicalPath === sourceIdentity.path) {
      throw new Error('Choose a different path when exporting a Canvas copy')
    }
    if (probe.kind === 'missing') await canvasDocumentCreate(targetPath, content)
    else await canvasDocumentSave(targetPath, content, probe.revision)
  })
}

export type DirtyChoice = 'save' | 'discard' | 'cancel'

export async function closeTab(
  id: string,
  confirm: (name: string) => Promise<DirtyChoice>,
): Promise<boolean> {
  const idx = tabs.findIndex((t) => t.id === id)
  if (idx < 0) return false
  const tab = tabs[idx]
  if (activeId.value === id) flushMountedDocument(id)
  if (isDirty(id)) {
    if (!tab.filePath) {
      // ── UNTITLED dirty file ──────────────────────────────────────────────
      // Go straight to the native NSSavePanel (no pre-ask).
      const { pickSaveCanvasFile, pickSaveFile } = await import('./dialogs')
      const p = tab.kind === 'canvas'
        ? await pickSaveCanvasFile('untitled.canvas')
        : await pickSaveFile()       // resolves to Documents/untitled.md
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
        await saveTab(id)                   // saves to existing path, no dialog
      }
      // choice === 'discard': fall through to close without saving
    }
  }
  tabs.splice(idx, 1)
  await stopWatchingTab(id)
  if (tab.kind === 'canvas') {
    const { releaseCanvasUiSession } = await import('./canvas/session')
    releaseCanvasUiSession(id)
  }
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
  let p = t.pendingExternal
  if (t.kind === 'canvas') {
    const { canvasDocumentOpen, canvasMtimeMs } = await import('./canvas/io')
    const opened = await canvasDocumentOpen(t.filePath)
    t.canvasRevision = opened.revision
    t.filePath = opened.canonicalPath
    t.title = basename(opened.canonicalPath)
    p = {
      content: opened.text,
      hash: opened.revision.sha256,
      mtime: canvasMtimeMs(opened.revision),
    }
  }
  const oldContent = t.currentContent
  t.initialContent = p.content
  t.currentContent = p.content
  t.lastKnownMtime = p.mtime
  t.lastKnownHash = p.hash
  t.externalState = 'fresh'
  t.externalBannerDismissed = false
  t.pendingExternal = undefined
  // 与 reloadTabFromDisk 一致地广播:OutlineEditor 只在这个事件上重建大纲树,
  // 少了它,横幅上点「重新加载」大纲不会刷新(RichEditor 另有 currentContent
  // 入站 effect 兜底,所以此前只有大纲这条链是断的)。
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('notemd:auto-reloaded', {
      detail: { tabId: t.id, oldContent, newContent: p.content },
    }))
  }
}

/**
 * Write the current buffer to disk, accepting the loss of the external
 * change. Clears banner state.
 */
export async function overwriteOnDisk(id: string): Promise<void> {
  const t = tabs.find((x) => x.id === id)
  if (!t || isManagedMemoryTab(t)) return
  if (shouldSkipEmptySave(t)) return
  if (t.kind === 'canvas') {
    await persistCanvasSnapshot(t, t.currentContent, true)
    return
  }
  await writeMd(t.filePath, t.currentContent)
  t.initialContent = t.currentContent
  await recordOurWrite(t)
}

/**
 * Persist one immutable Canvas snapshot through the Rust revision-checked,
 * same-directory atomic writer. The returned revision always tracks the bytes
 * that reached disk; the tab is marked clean only if no newer edit appeared
 * while the async write was in flight.
 */
interface CanvasDocumentIdentity {
  readonly path: string
  readonly revision?: CanvasDiskRevision
}

const canvasSaveQueues = new Map<string, Promise<unknown>>()

function cloneCanvasRevision(revision: CanvasDiskRevision | undefined): CanvasDiskRevision | undefined {
  return revision ? { ...revision } : undefined
}

function sameCanvasRevision(
  left: CanvasDiskRevision | undefined,
  right: CanvasDiskRevision | undefined,
): boolean {
  if (!left || !right) return left === right
  return left.mtimeNs === right.mtimeNs && left.size === right.size && left.sha256 === right.sha256
}

function captureCanvasIdentity(t: Tab): CanvasDocumentIdentity {
  return Object.freeze({ path: t.filePath, revision: cloneCanvasRevision(t.canvasRevision) })
}

function canvasIdentityMatches(t: Tab, identity: CanvasDocumentIdentity): boolean {
  return t.filePath === identity.path && sameCanvasRevision(t.canvasRevision, identity.revision)
}

/** Serialize every Canvas file operation that may change one tab's identity. */
function enqueueCanvasOperation<T>(
  t: Tab,
  run: (identity: CanvasDocumentIdentity) => Promise<T>,
): Promise<T> {
  const queueKey = t.id
  const previous = canvasSaveQueues.get(queueKey)
  const ready = previous ? previous.catch(() => undefined) : Promise.resolve()
  const operation = ready.then(() => run(captureCanvasIdentity(t)))
  const tracked = operation.finally(() => {
    if (canvasSaveQueues.get(queueKey) === tracked) canvasSaveQueues.delete(queueKey)
  })
  canvasSaveQueues.set(queueKey, tracked)
  return tracked
}

export function persistCanvasSnapshot(
  t: Tab,
  content: string,
  force = false,
  expectedPath?: string,
): Promise<CanvasSaveResult | undefined> {
  const immutableExpectedPath = expectedPath
  return enqueueCanvasOperation(t, (identity) => {
    if (immutableExpectedPath !== undefined && identity.path !== immutableExpectedPath) return Promise.resolve(undefined)
    return persistCanvasSnapshotNow(t, identity, content, force)
  })
}

async function persistCanvasSnapshotNow(
  t: Tab,
  identity: CanvasDocumentIdentity,
  content: string,
  force = false,
): Promise<CanvasSaveResult> {
  if (t.kind !== 'canvas' || !identity.path) throw new Error('Canvas save requires a path-backed Canvas tab')
  const {
    canvasDocumentCreate, canvasDocumentProbe, canvasDocumentSave, canvasMtimeMs,
  } = await import('./canvas/io')
  const wasDeleted = t.externalState === 'deleted'
  let saved: CanvasSaveResult
  try {
    if (wasDeleted) {
      saved = await canvasDocumentCreate(identity.path, content)
    } else {
      let revision = identity.revision
      if (!revision) {
        const probe = await canvasDocumentProbe(identity.path)
        if (probe.kind === 'missing') saved = await canvasDocumentCreate(identity.path, content)
        else {
          revision = probe.revision
          saved = await canvasDocumentSave(identity.path, content, revision, force)
        }
      } else {
        saved = await canvasDocumentSave(identity.path, content, revision, force)
      }
    }
  } catch (error) {
    const { asCanvasDocumentError, canvasDocumentOpen, canvasMtimeMs } = await import('./canvas/io')
    const detail = asCanvasDocumentError(error)
    if (detail?.kind === 'conflict' && canvasIdentityMatches(t, identity)) {
      if (detail.actual?.kind === 'missing') {
        t.externalState = 'deleted'
      } else {
        try {
          const disk = await canvasDocumentOpen(identity.path)
          t.externalState = 'changed'
          t.pendingExternal = {
            content: disk.text,
            hash: disk.revision.sha256,
            mtime: canvasMtimeMs(disk.revision),
          }
        } catch { /* preserve the original conflict */ }
      }
      t.externalBannerDismissed = false
    }
    throw error
  }
  // Do not let a completion for an identity that was rebound outside this
  // queue restore the old path/revision over the newer identity.
  if (canvasIdentityMatches(t, identity)) {
    t.canvasRevision = saved.revision
    t.filePath = saved.canonicalPath
    t.title = basename(saved.canonicalPath)
    t.lastKnownMtime = canvasMtimeMs(saved.revision)
    t.lastKnownHash = saved.revision.sha256
    t.externalState = 'fresh'
    t.externalBannerDismissed = false
    t.pendingExternal = undefined
    if (t.currentContent === content) t.initialContent = content
    if (wasDeleted) await rebindTabPath(t.id)
  }
  return saved
}

export function shouldSkipEmptySave(t: Tab): boolean {
  // 快速笔记草稿预置了 OKF 概念头,所以"空"= 去掉首部 frontmatter 后没有正文。
  return t.skipEmptySave === true && t.currentContent.replace(FM_BLOCK, '').trim().length === 0
}

/** 首部 YAML frontmatter 块(与 share-baker 同一形状)。 */
const FM_BLOCK = /^---\r?\n[\s\S]*?\r?\n---\r?\n?/

/**
 * Hide the banner without resolving the change. State stays non-fresh; the
 * banner reappears on the next external event.
 */
export function dismissExternalBanner(id: string): void {
  const t = tabs.find((x) => x.id === id)
  if (t) t.externalBannerDismissed = true
}
