import { untrack } from 'svelte'
import { readDir, watchImmediate, stat, exists, readTextFile, writeTextFile, remove } from '@tauri-apps/plugin-fs'
import { Store } from '@tauri-apps/plugin-store'
import { SvelteMap } from 'svelte/reactivity'
import { SvelteSet } from 'svelte/reactivity'
import { classifyPath, joinPath, type FileKind } from './fs'
import { dirname as parentDir, isWithinDir } from './paths'
import { companionPathFor } from './outline/store.svelte'
import type { SotRecord } from './sotvault-logic'

export interface FolderEntry {
  name: string
  path: string
  isDir: boolean
  kind: FileKind | null // null = directory or unsupported file type
  /** 独立大纲笔记(.note.md 无同名主文档):专属 note 图标 */
  isOutlineNote?: boolean
  /** 同目录存在配对 xxx.note.md:行尾角标,点击打开笔记 */
  hasNote?: boolean
  notePath?: string
  /** 伴生笔记中 agent 已作答、待人裁决的问题数(status:: answered 计数) */
  answeredCount?: number
  /** 最后修改(ms)，stat 失败为 0 */
  mtime?: number
  /** 创建(ms)，stat 失败为 0 */
  birthtime?: number
  /** 名字 ∈ 本目录 .notemd.json pinned 集 */
  pinned?: boolean
}

/** Parent directory of a file or directory path. Returns the path's own root
 *  ('/' on unix, 'D:/' on Windows) at the top.
 *  Imported *and* re-exported: `syncToActiveFile` below calls `parentDir`
 *  locally, and a bare `export … from` would not bind it in this scope. */
export { parentDir, isWithinDir }

/**
 * Compile a filter query into a name matcher. Empty/blank query → `null`
 * (meaning "match everything"). The query is treated as a case-insensitive
 * regular expression; an invalid pattern (e.g. a half-typed `(`) falls back to
 * a case-insensitive substring match so live typing keeps filtering sensibly.
 */
export function makeFilterMatcher(query: string): ((name: string) => boolean) | null {
  const q = query.trim()
  if (!q) return null
  try {
    const re = new RegExp(q, 'i')
    return (name) => re.test(name)
  } catch {
    const lower = q.toLowerCase()
    return (name) => name.toLowerCase().includes(lower)
  }
}

/**
 * Compute the set of paths that should stay visible under `query`, searching
 * the whole cached subtree under `rootDir`. A path is visible when:
 *   - it is a file whose name matches, or
 *   - it is a folder whose name matches (its entire subtree is then included), or
 *   - it is an ancestor folder on the path to any matching descendant.
 * Returns `null` when the query is empty (meaning "show everything").
 */
export function computeFilterVisibility(
  rootDir: string,
  cache: Map<string, FolderEntry[]>,
  query: string,
): Set<string> | null {
  const match = makeFilterMatcher(query)
  if (!match) return null
  const visible = new Set<string>()
  // `inherited` is true once an ancestor folder's name has matched, so its
  // whole subtree is included regardless of individual names.
  const walk = (entry: FolderEntry, inherited: boolean): boolean => {
    const selfMatch = inherited || match(entry.name)
    if (!entry.isDir) {
      if (selfMatch) visible.add(entry.path)
      return selfMatch
    }
    let anyChild = false
    for (const child of cache.get(entry.path) ?? []) {
      if (walk(child, selfMatch)) anyChild = true
    }
    const vis = selfMatch || anyChild
    if (vis) visible.add(entry.path)
    return vis
  }
  for (const e of cache.get(rootDir) ?? []) walk(e, false)
  return visible
}

const NOTE_SUFFIX_RE = /\.notes?\.md$/i

/** 伴生笔记文本里 agent 已作答、待人裁决的问题数。属性行 = 行内缩进 + status:: answered 独占一行(容忍 CRLF) */
export function countAnsweredQuestions(noteText: string): number {
  return (noteText.match(/^[ \t]+status:: answered\r?$/gm) ?? []).length
}

/** 同目录配对:xxx.note.md 有同名 xxx.md → 隐藏该行并给主行打 hasNote;
 *  无主文档的 .note.md 保留行并标 isOutlineNote。 */
export function pairNoteEntries(entries: FolderEntry[]): FolderEntry[] {
  const names = new Set(entries.filter(e => !e.isDir).map(e => e.name.toLowerCase()))
  const noteFor = new Map<string, FolderEntry>() // 主文件名(小写) → 笔记 entry
  for (const e of entries) {
    if (e.isDir || !NOTE_SUFFIX_RE.test(e.name)) continue
    const mainName = e.name.replace(NOTE_SUFFIX_RE, '.md').toLowerCase()
    if (names.has(mainName)) noteFor.set(mainName, e)
  }
  const out: FolderEntry[] = []
  for (const e of entries) {
    if (!e.isDir && NOTE_SUFFIX_RE.test(e.name)) {
      const mainName = e.name.replace(NOTE_SUFFIX_RE, '.md').toLowerCase()
      if (names.has(mainName)) continue            // 伴生:行隐藏
      out.push({ ...e, isOutlineNote: true })       // 独立笔记
      continue
    }
    const note = !e.isDir ? noteFor.get(e.name.toLowerCase()) : undefined
    out.push(note ? { ...e, hasNote: true, notePath: note.path } : e)
  }
  return out
}

/**
 * 源 md 的"有笔记"标识按 vault 判定:若某源文件的同步记录带非空 note_merge_base
 * (= vault 副本旁确有伴生 note),即使源目录本地没有 .note.md,也标 hasNote,
 * 且 notePath 指向 vault 副本旁的伴生 note(点击即打开 vault 里的笔记)。
 * 已本地配对(hasNote)或独立笔记的行不动。
 */
export function augmentVaultNotes(entries: FolderEntry[], records: SotRecord[]): FolderEntry[] {
  if (records.length === 0) return entries
  const vaultNoteBySource = new Map<string, string>()
  for (const r of records) {
    if (!r.note_merge_base) continue
    const note = companionPathFor(r.vault_path)
    if (note) vaultNoteBySource.set(r.source_path, note)
  }
  if (vaultNoteBySource.size === 0) return entries
  return entries.map((e) => {
    if (e.isDir || e.hasNote || e.isOutlineNote) return e
    if (!/\.md$/i.test(e.name) || NOTE_SUFFIX_RE.test(e.name)) return e
    const note = vaultNoteBySource.get(e.path)
    return note ? { ...e, hasNote: true, notePath: note } : e
  })
}

export type FolderSortKey = 'edited' | 'name' | 'created'
export const DEFAULT_SORT: FolderSortKey = 'edited'

/**
 * 排序：置顶组(按 pinned 数组序)在最前；其余"文件夹优先"，组内按 sort
 * (name 升序 / edited=mtime 倒序 / created=birthtime 倒序，时间相等回退名字)。
 */
export function sortEntries(
  entries: FolderEntry[],
  sort: FolderSortKey = DEFAULT_SORT,
  pinned: string[] = [],
): FolderEntry[] {
  const byName = (a: FolderEntry, b: FolderEntry) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
  const pinnedSet = new Set(pinned)
  const byNameMap = new Map(entries.map((e) => [e.name, e]))
  const pinnedGroup = pinned
    .map((n) => byNameMap.get(n))
    .filter((e): e is FolderEntry => !!e)
  const rest = entries.filter((e) => !pinnedSet.has(e.name))
  rest.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
    if (sort === 'name') return byName(a, b)
    if (sort === 'edited') return ((b.mtime ?? 0) - (a.mtime ?? 0)) || byName(a, b)
    return ((b.birthtime ?? 0) - (a.birthtime ?? 0)) || byName(a, b)
  })
  return [...pinnedGroup, ...rest]
}

export const PINNED_FILE = '.notemd.json'

/** 解析 .notemd.json 文本 → 置顶名字数组；任何异常/非法结构 → []。 */
export function parsePinned(text: string): string[] {
  try {
    const arr = (JSON.parse(text) as { pinned?: unknown })?.pinned
    return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

export type FolderViewMode = 'all' | 'withNotes' | 'markdown' | 'notes'
export const DEFAULT_VIEW_MODE: FolderViewMode = 'all'

const EXT_RE = /\.(md|markdown|mdown|mkd|mdx)$/i

/** 去 markdown 扩展名（无匹配原样返回）。 */
export function stripExt(name: string): string {
  return name.replace(EXT_RE, '')
}
/** 去伴生笔记后缀 .note.md / .notes.md（无匹配原样返回）。 */
export function stripNoteSuffix(name: string): string {
  return name.replace(/\.notes?\.md$/i, '')
}
/** 取正文第一个一级标题 `# xxx`；无则 null。front-matter 的 key 不会误判(不以 # 开头)。 */
export function parseFirstH1(text: string): string | null {
  const m = text.match(/^#\s+(.+?)\s*$/m)
  return m ? m[1] : null
}

/** 按视图模式过滤条目（各模式均保留文件夹供导航；隐藏文件夹由 applyHideFolders 独立处理）。 */
export function filterByViewMode(entries: FolderEntry[], mode: FolderViewMode): FolderEntry[] {
  switch (mode) {
    case 'withNotes': return entries.filter((e) => e.isDir || e.hasNote === true)
    case 'markdown': return entries.filter((e) => e.isDir || e.kind === 'markdown' || e.kind === 'mdx')
    case 'notes': return entries.filter((e) => e.isDir || e.isOutlineNote === true || e.hasNote === true)
    default: return entries
  }
}

/** 「隐藏文件夹」独立过滤（与任意视图模式并行叠加）。 */
export function applyHideFolders(entries: FolderEntry[], hide: boolean): FolderEntry[] {
  if (!hide) return entries
  return entries.filter((e) => !e.isDir)
}

/** 视图模式下的显示名（只改可见文字）。markdown=H1/去扩展；notes=去后缀。 */
export function displayNameFor(entry: FolderEntry, mode: FolderViewMode, title?: string | null): string {
  if (entry.isDir) return entry.name
  if (mode === 'markdown') return title && title.length ? title : stripExt(entry.name)
  if (mode === 'notes') {
    if (entry.isOutlineNote) return stripNoteSuffix(entry.name)
    if (entry.hasNote) return stripExt(entry.name)
  }
  return entry.name
}

export interface FolderViewState {
  enabled: boolean
  visible: boolean
  width: number
  rootDir: string | null
  /** Live name filter (regex, case-insensitive); empty = no filtering. */
  filter: string
  /** Paths kept visible by the active filter (recursive; empty when no filter). */
  filterVisible: SvelteSet<string>
  expanded: SvelteSet<string>
  entriesCache: SvelteMap<string, FolderEntry[]>
  /** 全局排序方式（存 settings.json） */
  sort: FolderSortKey
  /** 单选视图模式（渲染过滤，存 settings.json） */
  viewMode: FolderViewMode
  /** 隐藏文件夹（独立复选，与视图模式并行，存 settings.json） */
  hideFolders: boolean
  /** markdown 模式 H1 惰性缓存：path → { mtime, title|null } */
  titleCache: SvelteMap<string, { mtime: number; title: string | null }>
}

export const DEFAULT_WIDTH = 240
export const MIN_WIDTH = 160
export const MAX_WIDTH = 480

export const folderView = $state<FolderViewState>({
  enabled: true,
  visible: false,
  width: DEFAULT_WIDTH,
  rootDir: null,
  filter: '',
  filterVisible: new SvelteSet(),
  expanded: new SvelteSet(),
  entriesCache: new SvelteMap(),
  sort: DEFAULT_SORT,
  viewMode: DEFAULT_VIEW_MODE,
  hideFolders: false,
  titleCache: new SvelteMap(),
})

/** 读本目录 .notemd.json → 置顶名字数组；无文件/异常 → []（绝不创建）。 */
export async function readPinned(dir: string): Promise<string[]> {
  const path = joinPath(dir, PINNED_FILE)
  if (!(await exists(path).catch(() => false))) return []
  return parsePinned(await readTextFile(path).catch(() => ''))
}

/** 切换置顶：读→改→写；结果空则删文件；随后重读本目录刷新缓存。 */
export async function togglePin(dir: string, name: string): Promise<void> {
  const path = joinPath(dir, PINNED_FILE)
  const cur = await readPinned(dir)
  const next = cur.includes(name) ? cur.filter((n) => n !== name) : [...cur, name]
  if (next.length === 0) {
    if (await exists(path).catch(() => false)) await remove(path).catch(() => {})
  } else {
    await writeTextFile(path, JSON.stringify({ pinned: next }, null, 2) + '\n').catch(() => {})
  }
  await readFolder(dir).catch(() => {})
}

/** Read a directory: classify, stat(time), read pins, mark, sort, cache. */
export async function readFolder(dir: string): Promise<FolderEntry[]> {
  const raw = await readDir(dir)
  const base: FolderEntry[] = raw
    .filter((e) => !e.name.startsWith('.'))
    .map((e) => {
      const path = joinPath(dir, e.name)
      return {
        name: e.name,
        path,
        isDir: !!e.isDirectory,
        kind: e.isDirectory ? null : (classifyPath(path)?.kind ?? null),
      }
    })
  await Promise.all(base.map(async (en) => {
    const st = await stat(en.path).catch(() => null)
    en.mtime = st?.mtime ? new Date(st.mtime).getTime() : 0
    en.birthtime = st?.birthtime ? new Date(st.birthtime).getTime() : 0
  }))
  const pinned = await readPinned(dir)
  const pinnedSet = new Set(pinned)
  const { sotvaultStore } = await import('./sotvault.svelte')
  const withVaultNotes = augmentVaultNotes(pairNoteEntries(base), sotvaultStore.records)
  await Promise.all(withVaultNotes.map(async (e) => {
    if (!e.hasNote || !e.notePath) return
    const txt = await readTextFile(e.notePath!).catch(() => null)
    if (txt) {
      const c = countAnsweredQuestions(txt)
      if (c > 0) e.answeredCount = c
    }
  }))
  const paired = withVaultNotes.map((e) => (pinnedSet.has(e.name) ? { ...e, pinned: true } : e))
  const sorted = sortEntries(paired, folderView.sort, pinned)
  folderView.entriesCache.set(dir, sorted)
  return sorted
}

/** Reveal a file/folder in the OS file browser (Finder), selecting the item. */
export async function revealInFinder(path: string): Promise<void> {
  const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
  await revealItemInDir(path)
}

/** Set the tree root and eagerly read it. */
export async function setRootDir(dir: string): Promise<void> {
  folderView.rootDir = dir
  folderView.expanded.clear()
  await readFolder(dir).catch(() => {})
  // Re-run an active filter against the new root's subtree.
  if (folderView.filter.trim()) await setFilter(folderView.filter)
}

/**
 * 启动 / 配置 vault 后:若文件夹视图还没有根(空白启动、没有活动文件建立根),
 * 默认把根指向 vault,让空窗口也能在文件夹视图里看到 vault 内容。已有根则不动
 * ——不覆盖用户当前的浏览位置,也不跟已 reveal 的文件抢根。vault 未配置时 no-op。
 */
export async function defaultRootToVault(vaultRoot: string | null): Promise<void> {
  if (!vaultRoot) return
  if (folderView.rootDir != null) return
  await setRootDir(vaultRoot)
}

/** 上一次跟随过的活动文件路径:同一文件重复同步不再抢用户手动导航的位置。 */
let lastFollowedPath: string | null = null
/** 一次性抑制:文件夹视图树内点开的文件不改根(那本身就是在 view 里浏览)。 */
let pendingSuppressPath: string | null = null

/** 树内打开前登记:紧接着的这一次跟随跳过换根。标记只消费一次。 */
export function suppressFollowFor(path: string): void {
  pendingSuppressPath = path
}

/** 清跟随状态(模块级,单测用于隔离用例)。 */
export function resetFollowState(): void {
  lastFollowedPath = null
  pendingSuppressPath = null
}

/**
 * React to the active file changing: the root follows the file's own directory,
 * even when the file already sits inside the current root's subtree (the tree
 * doesn't auto-expand ancestors, so a deep file would otherwise be invisible).
 *
 * 两个例外:
 *   - 活动文件没变(面板重挂载、无关状态刷新)→ 保留用户手动导航(↑)的位置;
 *   - 这次打开来自文件夹视图树内(`suppressFollowFor`)→ 不让树在脚下跳动。
 *
 * `folderView.rootDir` 的读取走 untrack:本函数在 $effect 里同步调用,而
 * `setRootDir` 会写同一批状态,读+写会自失效成死循环。
 */
export async function syncToActiveFile(filePath: string | null): Promise<void> {
  if (!filePath) return
  if (filePath === lastFollowedPath) return
  lastFollowedPath = filePath
  if (pendingSuppressPath === filePath) {
    pendingSuppressPath = null
    return
  }
  const parent = parentDir(filePath)
  if (untrack(() => folderView.rootDir) === parent) return
  await setRootDir(parent)
}

// Bumped on every filter change so a slow recursive load can tell it has been
// superseded by newer input and skip its (now stale) recompute.
let filterSeq = 0

/**
 * Set the live name filter (not persisted). Loads the whole subtree under the
 * root so matches in any subfolder are found, then recomputes visibility.
 */
export async function setFilter(q: string): Promise<void> {
  folderView.filter = q
  const seq = ++filterSeq
  if (!q.trim() || !folderView.rootDir) {
    folderView.filterVisible.clear()
    return
  }
  await ensureSubtreeLoaded(folderView.rootDir)
  if (seq !== filterSeq) return // a newer query took over while we loaded
  recomputeFilter()
}

/** Clear the name filter and cancel any in-flight recursive load. */
export function clearFilter(): void {
  folderView.filter = ''
  folderView.filterVisible.clear()
  filterSeq++
}

/** Recompute `filterVisible` from the current cache + filter. */
function recomputeFilter(): void {
  const vis = folderView.rootDir
    ? computeFilterVisibility(folderView.rootDir, folderView.entriesCache, folderView.filter)
    : null
  folderView.filterVisible.clear()
  if (vis) for (const p of vis) folderView.filterVisible.add(p)
}

/** Recursively read every folder under `dir` (skipping already-cached ones). */
async function ensureSubtreeLoaded(dir: string): Promise<void> {
  if (!folderView.entriesCache.has(dir)) await readFolder(dir).catch(() => {})
  const dirs = (folderView.entriesCache.get(dir) ?? []).filter((e) => e.isDir)
  await Promise.all(dirs.map((e) => ensureSubtreeLoaded(e.path)))
}

/** Expand/collapse a folder; read its children on first expand. */
export async function toggleExpanded(dir: string): Promise<void> {
  if (folderView.expanded.has(dir)) {
    folderView.expanded.delete(dir)
  } else {
    folderView.expanded.add(dir)
    if (!folderView.entriesCache.has(dir)) await readFolder(dir).catch(() => {})
  }
}

/** Re-read every directory currently cached (manual refresh). */
export async function refreshAll(): Promise<void> {
  const dirs = [...folderView.entriesCache.keys()]
  await Promise.all(dirs.map(async (d) => {
    try {
      await readFolder(d)
    } catch {
      // Unreadable = renamed/deleted: keeping the stale entries would leave a
      // ghost directory in the tree until restart. It re-reads on demand if
      // the failure was transient.
      folderView.entriesCache.delete(d)
      folderView.expanded.delete(d)
    }
  }))
  // Keep the active filter current: pick up newly added subfolders/files.
  if (folderView.filter.trim() && folderView.rootDir) {
    await ensureSubtreeLoaded(folderView.rootDir)
    recomputeFilter()
  }
}

/**
 * Watch `dir` recursively and re-read the visible tree (debounced) whenever the
 * filesystem changes under it, so newly created / deleted / renamed files show
 * up without a manual refresh. Returns an unwatch function that's safe to call
 * even while the underlying watch is still starting.
 */
export function watchRoot(dir: string): () => void {
  let stop: (() => void) | null = null
  let cancelled = false
  let timer: ReturnType<typeof setTimeout> | null = null
  const schedule = () => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => { void refreshAll() }, 150)
  }
  watchImmediate(dir, schedule, { recursive: true })
    .then((s) => { if (cancelled) { try { s() } catch { /* ignore */ } } else { stop = s } })
    .catch((e) => console.warn('[folder-view] watch failed for', dir, e))
  return () => {
    cancelled = true
    if (timer) { clearTimeout(timer); timer = null }
    if (stop) { try { stop() } catch (e) { console.warn('[folder-view] unwatch failed:', e) } stop = null }
  }
}

// ---- persistence (settings.json store; shared with settings.svelte.ts) ----

let store: Awaited<ReturnType<typeof Store.load>> | null = null
async function getStore() {
  if (!store) store = await Store.load('settings.json')
  return store
}

export async function loadFolderViewState(): Promise<void> {
  // Core-ized: always enabled; no plugin gate.
  folderView.enabled = true
  const s = await getStore()
  folderView.visible = (await s.get<boolean>('folderView.visible')) ?? false
  folderView.width = (await s.get<number>('folderView.width')) ?? DEFAULT_WIDTH
  const savedSort = await s.get<string>('folderView.sort')
  folderView.sort = savedSort === 'name' || savedSort === 'created' || savedSort === 'edited' ? savedSort : DEFAULT_SORT
  const savedMode = await s.get<string>('folderView.viewMode')
  folderView.viewMode = savedMode && ['all', 'withNotes', 'markdown', 'notes'].includes(savedMode)
    ? (savedMode as FolderViewMode)
    : (await s.get<boolean>('folderView.notesOnly')) ? 'withNotes' : DEFAULT_VIEW_MODE
  // 隐藏文件夹：显式设置优先；否则从旧的 files 视图/filesOnly 迁移
  folderView.hideFolders = (await s.get<boolean>('folderView.hideFolders'))
    ?? (savedMode === 'files' || ((await s.get<boolean>('folderView.filesOnly')) ?? false))
}

/** 设置全局排序方式：就地重排所有已缓存目录（时间元数据已在 entry 上，无需重读盘）。 */
export async function setSort(key: FolderSortKey): Promise<void> {
  folderView.sort = key
  for (const [dir, entries] of folderView.entriesCache) {
    const pinned = entries.filter((e) => e.pinned).map((e) => e.name)
    folderView.entriesCache.set(dir, sortEntries(entries, key, pinned))
  }
  const s = await getStore()
  await s.set('folderView.sort', key)
  await s.save()
}

/** 设置单选视图模式（渲染过滤，不重读盘）。 */
export async function setViewMode(mode: FolderViewMode): Promise<void> {
  folderView.viewMode = mode
  const s = await getStore()
  await s.set('folderView.viewMode', mode)
  await s.save()
}

/** 设置「隐藏文件夹」（独立复选，不重读盘）。 */
export async function setHideFolders(v: boolean): Promise<void> {
  folderView.hideFolders = v
  const s = await getStore()
  await s.set('folderView.hideFolders', v)
  await s.save()
}

/** markdown 模式惰性读某 md 的首个 H1 → titleCache（按 mtime 去重）。 */
export async function ensureTitle(entry: FolderEntry): Promise<void> {
  const mtime = entry.mtime ?? 0
  const cached = folderView.titleCache.get(entry.path)
  if (cached && cached.mtime === mtime) return
  let title: string | null = null
  try {
    title = parseFirstH1(await readTextFile(entry.path))
  } catch { title = null }
  folderView.titleCache.set(entry.path, { mtime, title })
}

/** @deprecated visibility/width now live in the side-panel registry (sidePanels.left).
 *  Retained for folder-view.test.ts + settings hydration; the UI no longer reads them. */
export async function setVisible(v: boolean): Promise<void> {
  folderView.visible = v
  const s = await getStore()
  await s.set('folderView.visible', v)
  await s.save()
}

export async function setWidth(w: number): Promise<void> {
  const clamped = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(w)))
  folderView.width = clamped
  const s = await getStore()
  await s.set('folderView.width', clamped)
  await s.save()
}
