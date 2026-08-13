// src/lib/outline/backlinks.ts
import { parseInline, eachInline } from './parser'
import { parseOutline } from './markdown'
import type { OutlineTree } from './model'
import { basename, joinPath, relative } from '../paths'

export interface BacklinkHit { file: string; text: string; line: number }

/** 页面命名空间：root 下第一段目录 ∈ dirs 的 .md 才算 wiki 页（递归）。 */
export interface PageScope { root: string; dirs: string[] }

export interface BacklinkIndex {
  /** lowercased target → hits */
  byTarget: Map<string, BacklinkHit[]>
  /** file → its targets（增量更新用） */
  fileTargets: Map<string, Set<string>>
  /** 已索引「wiki 页」的页面名（[[ 补全候选 / 解析目标） */
  filePages: Map<string, string>
  /** 页面命名空间；null = 所有 .md 都是页面（向后兼容） */
  scope: PageScope | null
  /** file → 解析后的大纲树（仅含 ≥1 个链接的文件）。Linked References 层次
   *  召回复用此缓存，避免视图时重复读盘 + 重新 parseOutline。watcher 增量维护。 */
  fileTrees: Map<string, OutlineTree>
}

export function createIndex(scope: PageScope | null = null): BacklinkIndex {
  return { byTarget: new Map(), fileTargets: new Map(), filePages: new Map(), scope, fileTrees: new Map() }
}

/**
 * path 是否为「wiki 页」：相对 scope.root 的第一段 ∈ scope.dirs 且以 .md 结尾（递归子目录都算）。
 * scope 为 null → 所有 .md 都是页面（纯逻辑调用 / 向后兼容）。
 */
export function isWikiPagePath(scope: PageScope | null, path: string): boolean {
  if (!/\.md$/i.test(path)) return false
  if (!scope) return true
  const rel = relative(scope.root, path)
  if (rel === null) return false
  const segs = rel.split('/')
  return segs.length >= 2 && scope.dirs.includes(segs[0])
}

export function pageNameOf(path: string): string {
  return basename(path).replace(/\.notes?\.md$/i, '').replace(/\.md$/i, '')
}

export function removeFileFromIndex(idx: BacklinkIndex, file: string): void {
  const targets = idx.fileTargets.get(file)
  if (targets) {
    for (const t of targets) {
      const hits = idx.byTarget.get(t)?.filter(h => h.file !== file) ?? []
      if (hits.length) idx.byTarget.set(t, hits)
      else idx.byTarget.delete(t)
    }
  }
  idx.fileTargets.delete(file)
  idx.filePages.delete(file)
  idx.fileTrees.delete(file)
}

/** 单文件（重新）索引：逐行提取 [[..]] 与 #tag */
export function indexFileContent(idx: BacklinkIndex, file: string, content: string): void {
  removeFileFromIndex(idx, file)
  if (isWikiPagePath(idx.scope, file)) idx.filePages.set(file, pageNameOf(file))
  const targets = new Set<string>()
  content.split('\n').forEach((rawLine, i) => {
    const text = rawLine.replace(/^\s*- /, '').trim()
    if (!text) return
    for (const node of eachInline(parseInline(text))) {
      let target: string | null = null
      if (node.t === 'page-link') target = node.target
      else if (node.t === 'hashtag') target = node.tag
      if (!target) continue
      const key = target.toLowerCase()
      targets.add(key)
      const hits = idx.byTarget.get(key) ?? []
      hits.push({ file, text, line: i + 1 })
      idx.byTarget.set(key, hits)
    }
  })
  idx.fileTargets.set(file, targets)
  // Cache the parsed outline for hierarchy-aware recall (only linking files can
  // appear in Linked References). removeFileFromIndex() above cleared any stale one.
  if (targets.size > 0) idx.fileTrees.set(file, parseOutline(content))
}

export function backlinksFor(idx: BacklinkIndex, page: string): BacklinkHit[] {
  return idx.byTarget.get(page.toLowerCase()) ?? []
}

export function pageCandidates(idx: BacklinkIndex): string[] {
  return [...new Set(idx.filePages.values())]
}

/** p 是否为"伴生笔记"(同目录存在同名主文档,均已入索引) */
function isCompanionIn(idx: BacklinkIndex, p: string): boolean {
  return /\.notes?\.md$/i.test(p) && idx.filePages.has(p.replace(/\.notes?\.md$/i, '.md'))
}

/**
 * [[target]] → 文件路径(spec §5,file-over-app 修订):只按文件名(大小写不敏感)。
 * 主文档(.md)优先;独立 .note.md(wiki 页)可为目标;伴生 .note.md 永不为目标。
 * 无命中返回 null。
 */
export function resolveTarget(idx: BacklinkIndex, target: string): string | null {
  const t = target.toLowerCase()
  const hits = [...idx.filePages.entries()].filter(([, page]) => page.toLowerCase() === t)
  if (hits.length === 0) return null
  const md = hits.find(([p]) => !/\.notes?\.md$/i.test(p))
  if (md) return md[0]
  const standalone = hits.find(([p]) => !isCompanionIn(idx, p))
  return standalone ? standalone[0] : null
}

/**
 * 文件名碰撞检测(spec §5):同一链接名被多个文件竞争(伴生笔记不算,
 * 它与主文档同名是格式约定)。返回 小写页名 → 冲突文件列表(仅 >1 时收录)。
 */
export function detectNameCollisions(idx: BacklinkIndex): Map<string, string[]> {
  const byName = new Map<string, string[]>()
  for (const [p, page] of idx.filePages.entries()) {
    if (isCompanionIn(idx, p)) continue
    const key = page.toLowerCase()
    byName.set(key, [...(byName.get(key) ?? []), p])
  }
  const out = new Map<string, string[]>()
  for (const [k, v] of byName) if (v.length > 1) out.set(k, v)
  return out
}

// ---------- IO（组件层调用；vitest 不覆盖，走手动验证） ----------

const MAX_FILE_BYTES = 1024 * 1024 // spec 性能护栏：仅解析 ≤1MB

/** 扫描 rootDir 下所有 .note.md 建全量索引（递归、跳过点目录/点文件）。
 *  纯 .md 一律跳过（尺寸不可控）。
 *  副作用:遇到旧后缀 *.notes.md 会就地迁移改名为 *.note.md(冲突时回调上报)。 */
export async function buildFolderIndex(
  rootDir: string,
  dirs: string[],
  onMigrateConflict?: (legacyPath: string) => void,
): Promise<BacklinkIndex> {
  const { readDir, readTextFile, stat } = await import('@tauri-apps/plugin-fs')
  const idx = createIndex({ root: rootDir, dirs })
  const walk = async (dir: string): Promise<void> => {
    const entries = await readDir(dir).catch(() => [])
    for (const e of entries) {
      if (e.name.startsWith('.')) continue
      if (e.isSymlink) continue // skip symlinks to avoid cycle risk
      let path = joinPath(dir, e.name)
      if (e.isDirectory) { await walk(path); continue }
      // 只索引 .note.md（含旧后缀 .notes.md，会就地迁移）：note 文件由 app 自管、
      // 尺寸可控；纯 .md（导入稿/转录/摘要等）尺寸不可控，一律不扫也不登记为页面。
      if (!/\.notes?\.md$/i.test(e.name)) continue
      if (/\.notes\.md$/i.test(e.name)) {
        const { migrateLegacyFile, migratedPathFor } = await import('./migrate')
        const r = await migrateLegacyFile(path)
        if (r === 'renamed') path = migratedPathFor(path)!
        else if (r === 'conflict') onMigrateConflict?.(path)
      }
      const info = await stat(path).catch(() => null)
      if (info && info.size > MAX_FILE_BYTES) continue
      const content = await readTextFile(path).catch(() => null)
      if (content != null) indexFileContent(idx, path, content)
    }
  }
  await walk(rootDir)
  return idx
}

/**
 * watcher 事件分类:目录改名/移动在 FSEvents 上只报目录自身路径,不逐个报
 * 子文件 —— 只按 `.note.md` 过滤会把整个事件吞掉,索引留旧路径。末段无 `.`
 * 且不以 `.` 开头的路径按目录对待(`notes.v2` 这类带点目录名认不出,漏检的
 * 代价只是等下一次全量重建,不做 stat 换精度)。
 */
export function classifyWatchPaths(paths: string[]): { notes: string[]; dirChange: boolean } {
  const notes: string[] = []
  let dirChange = false
  for (const p of paths) {
    if (/\.notes?\.md$/i.test(p)) { notes.push(p); continue }
    const norm = p.replace(/[\\/]+$/, '')
    if (norm.split(/[\\/]/).some((s) => s.startsWith('.'))) continue
    const seg = norm.split(/[\\/]/).pop() ?? ''
    if (seg && !seg.includes('.')) dirChange = true
  }
  return { notes, dirChange }
}

/** file-watcher 事件驱动的单文件增量重扫（仅 .note.md，纯 .md 不索引） */
export async function refreshFileInIndex(idx: BacklinkIndex, path: string): Promise<void> {
  if (!/\.notes?\.md$/i.test(path)) return
  const { readTextFile } = await import('@tauri-apps/plugin-fs')
  const content = await readTextFile(path).catch(() => null)
  if (content == null) removeFileFromIndex(idx, path)
  else indexFileContent(idx, path, content)
}
