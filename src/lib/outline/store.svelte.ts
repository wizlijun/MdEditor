// src/lib/outline/store.svelte.ts
import { createTree, childrenOf, treeHasQuestion, type OutlineTree, type OutlineNode } from './model'
import { serializeOutline, parseOutline } from './markdown'
import { collapseAnsweredOnLoad } from './auto-collapse'
import { deriveAutoItems } from './derive'
import { syncAutoItems, regenerate as regenerateTree } from './sync'
import { parseInline, eachInline } from './parser'
import type { BacklinkIndex } from './backlinks'
import { pageNameOf } from './backlinks'
import { touchFrontmatter, fmHas, outlineConceptType } from './frontmatter'
import { outlineDirs } from './dirs.svelte'
import { sourcesForNote } from './note-source'

export interface OutlineState {
  /** 全屏大纲 tab 模式:当前挂载的 .note.md 路径 */
  docPath: string | null
  tree: OutlineTree
  /** 触发 Svelte 重渲染的版本号：任何树结构/内容变更后 bump */
  version: number
  editingId: string | null
  /** 多选集合。每次变更必须整体重赋值（Set 内部变异不触发响应） */
  selectedIds: Set<string>
  /** Shift 连选的锚点：最近一次点击/进入编辑的节点 */
  selectionAnchor: string | null
  backlinkIndex: BacklinkIndex | null
  /** 相对上次落盘是否有未保存变更（驱动保存按钮指示态） */
  dirty: boolean
  /** 自动保存是否已激活（磁盘已有内容 / 用户编辑过 / 点过保存） */
  armed: boolean
  /** panel 模式写盘前发现远端已改动的冲突态；非 null 时禁止自动写盘 */
  externalConflict: { diskText: string } | null
  /** 折叠状态外置(每日笔记窗口):serializeDoc 省略 collapsed::,折叠只存 KV,
   *  不写进 .note.md。默认 false,主窗口行为不变。 */
  omitCollapsed: boolean
  /** 仅视图态的折叠(答复已到的问题默认折叠);序列化时跳过,绝不写回文件 */
  autoCollapsed: Set<string>
  /** 允许把非空 note 主动删空并写回(每日笔记窗口):关闭 change-sink 的
   *  "空树不覆盖非空"守卫的空树分支(仍保留 docPath!==path 竞态保护)。
   *  默认 false,主窗口保留原防误抹行为。 */
  allowEmptyWrite: boolean
}

export const outline = $state<OutlineState>({
  docPath: null,
  tree: createTree(),
  version: 0,
  editingId: null,
  selectedIds: new Set(),
  selectionAnchor: null,
  backlinkIndex: null,
  dirty: false,
  armed: false,
  externalConflict: null,
  omitCollapsed: false,
  allowEmptyWrite: false,
  autoCollapsed: new Set(),
})

export function bump(): void { outline.version++ }

export function setSelection(ids: Iterable<string>): void {
  outline.selectedIds = new Set(ids)
}

export function clearSelection(): void {
  if (outline.selectedIds.size > 0) outline.selectedIds = new Set()
}

/** 新旧两种大纲后缀(迁移期兼容识别) */
export const OUTLINE_SUFFIX_RE = /\.notes?\.md$/i

export function companionPathFor(mainPath: string): string | null {
  if (OUTLINE_SUFFIX_RE.test(mainPath)) return null
  // mdx 保留完整后缀(foo.mdx → foo.mdx.note.md):同目录下 index.md 与
  // index.mdx 共存不算罕见,去后缀会让两份文档共用一份手记且无法分开。
  if (/\.mdx$/i.test(mainPath)) return `${mainPath}.note.md`
  const m = mainPath.match(/^(.*)\.(md|markdown|mdown|mkd)$/i)
  return m ? `${m[1]}.note.md` : null
}

/** 需要写 id:: 的节点：被 ((ref)) 引用的 + 带手写子节点的 auto 节点 */
export function persistIdsFor(tree: OutlineTree): Set<string> {
  const ids = new Set<string>()
  for (const n of tree.nodes.values()) {
    for (const seg of eachInline(parseInline(n.content))) {
      if (seg.t === 'block-ref' && tree.nodes.has(seg.refId)) ids.add(seg.refId)
    }
    if (n.source !== 'manual' && childrenOf(tree, n.id).some(c => c.source === 'manual')) ids.add(n.id)
  }
  return ids
}

/** copy-ref 时固定写入 id 的集合：确保即使引用被粘到别的文件，本文件也会落盘 id:: */
export const pinnedIds = new Set<string>()

/**
 * 数据丢失防线原语。全局树是单例，attach/detach 竞态下 sink 可能在"空树 / 只有
 * 一个自动创建的空白根节点"时触发序列化——把真实伴生笔记覆盖掉。
 *
 * `isEffectivelyEmptyTree`：没有任何用户可辨识内容的树。写回前若树是这种状态、
 * 而落点(缓冲/磁盘)本来非空，必须拒绝写入（见 OutlineEditor 的 sink 守卫）。
 */
export function isEffectivelyEmptyTree(tree: OutlineTree): boolean {
  if (tree.nodes.size === 0) return true
  if (tree.nodes.size === 1) {
    const only = tree.nodes.values().next().value as OutlineNode
    return only.source === 'manual' && only.content.trim() === ''
  }
  return false
}

/** 伴生笔记 markdown 里是否有"带文字的条目"——空/空白/仅 front-matter/空 bullet 都算无内容。 */
export function noteTextHasContent(text: string): boolean {
  return /^\s*-\s+\S/m.test(text)
}

// ---------- 全屏大纲 tab 模式(phase 2):IO 由 tabs 体系接管,这里只有内存树 ----------

let changeSink: (() => void) | null = null
/** 镜像源解析器。由 App 注入(store 不直接依赖 sotvault,避免循环依赖);
 *  未注入时伴生笔记就不带 sources —— 缺字段永远比写错字段好。 */
let mirrorSourceResolver: ((mainPath: string) => string | null) | null = null
export function setMirrorSourceResolver(fn: ((mainPath: string) => string | null) | null): void {
  mirrorSourceResolver = fn
}

/** 编辑器注册:任何树变更(markDirty)后被调用,负责 serializeDoc → setContent(tab) */
export function setChangeSink(fn: (() => void) | null): void { changeSink = fn }

/**
 * 挂载一篇 .note.md 文本到全局树。mainContent 非 null(伴生笔记)时对主文档
 * 跑一次派生同步(spec §4:派生移到大纲 tab 挂载时)。不写盘、不触发 sink。
 */
export async function attachDoc(docPath: string, text: string, mainContent: string | null): Promise<void> {
  outline.docPath = docPath
  outline.tree = parseOutline(text)
  // 答复已到的问题默认折叠:刚跑完 agent 的笔记否则全是 ✦ 正文,扫不动。
  outline.autoCollapsed = collapseAnsweredOnLoad(outline.tree)
  outline.editingId = null
  outline.selectedIds = new Set()
  outline.selectionAnchor = null
  // 存量文件缺 created → 补文件 birthtime(测试环境 stat 不可用,静默跳过)
  if (!fmHas(outline.tree.frontmatter, 'created')) {
    const info = await import('@tauri-apps/plugin-fs')
      .then(m => m.stat(docPath)).catch(() => null)
    if (outline.docPath !== docPath) return   // superseded by a later attachDoc
    if (info?.birthtime) {
      outline.tree.frontmatter = touchFrontmatter(outline.tree.frontmatter, {
        title: pageNameOf(docPath), created: new Date(info.birthtime).toISOString(),
        type: outlineConceptType(docPath, outlineDirs),
      })
    }
  }
  outline.armed = noteTextHasContent(text)
  outline.dirty = false
  outline.externalConflict = null
  if (mainContent != null) syncAutoItems(outline.tree, deriveAutoItems(mainContent))
  // 提问视为明确保存意图:树中出现 question 节点即激活落盘(spec §2)
  if (treeHasQuestion(outline.tree)) outline.armed = true
  bump()
}

/**
 * 序列化当前树。touch=true(默认,编辑 sink 路径)刷新 front-matter 的
 * updated 并补齐 title;touch=false 用于挂载时的无副作用对比,避免
 * "打开即脏"(updated 刷新会让未编辑的 tab 变 dirty)。
 * 注:setContent 只是赋值 tab.currentContent,不会回调 markDirty,无再入循环。
 */
export function serializeDoc(touch = true): string {
  if (outline.docPath && touch) {
    outline.tree.frontmatter = touchFrontmatter(outline.tree.frontmatter, {
      title: pageNameOf(outline.docPath),
      type: outlineConceptType(outline.docPath, outlineDirs),
      sources: mirrorSourceResolver
        ? sourcesForNote(outline.docPath, mirrorSourceResolver)
        : undefined,
    })
  }
  return serializeOutline(
    outline.tree,
    new Set([...persistIdsFor(outline.tree), ...pinnedIds]),
    outline.omitCollapsed,
    outline.autoCollapsed,
  )
}

/** 卸载当前文档:清 docPath/树/选区。全屏大纲 tab 关闭时由编辑器调用。 */
export function detach(): void {
  outline.docPath = null
  outline.tree = createTree()
  outline.editingId = null
  outline.selectedIds = new Set()
  outline.selectionAnchor = null
  outline.dirty = false
  outline.armed = false
  outline.externalConflict = null
  bump()
}

export function regenerate(mainContent: string): void {
  regenerateTree(outline.tree, deriveAutoItems(mainContent))
  bump()
  markDirty()
}

/** 用户编辑：置脏 + 激活自动保存 + 通知 sink 落盘。 */
export function markDirty(): void {
  outline.dirty = true
  outline.armed = true
  changeSink?.()
}

/** 同步派生：只置脏；仅在已激活时才落盘（浏览/主文档编辑不自动生成笔记）。 */
export function markSynced(): void {
  outline.dirty = true
  if (outline.armed) changeSink?.()
}

/** 落盘成功后清脏（写盘点回调）。 */
export function markSaved(): void {
  outline.dirty = false
}
