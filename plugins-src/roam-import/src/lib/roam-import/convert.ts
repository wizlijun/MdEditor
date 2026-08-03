// src/lib/roam-import/convert.ts
import { createTree, addNode, newId, type OutlineNode } from '../outline/model'
import { serializeOutline } from '../outline/markdown'
import { touchFrontmatter } from '../outline/frontmatter'
import { convertInline, rewriteLinks, normalizeDateLinks, escapeStructuralLines, closeDanglingFence } from './syntax'
import { dailyDateFromUid } from './parse'
import type { RoamBlock, RoamPage } from './types'

export interface ConvertedPage {
  title: string
  text: string
  /** 页面级增量判定时间:页与全部 block edit-time 的最大值 */
  editTime: number
}

export function maxEditTime(page: RoamPage): number {
  let max = page['edit-time'] ?? 0
  const walk = (bs: RoamBlock[] | undefined) => {
    for (const b of bs ?? []) {
      if ((b['edit-time'] ?? 0) > max) max = b['edit-time']!
      walk(b.children)
    }
  }
  walk(page.children)
  return max
}

function iso(ms: number | undefined): string | undefined {
  return ms != null ? new Date(ms).toISOString() : undefined
}

/**
 * Roam block 文本 → 一个 outline 节点的 content。三种会被 parseOutline 读成结构的
 * 形状要中和:属性行、子 bullet 行(两者由 escapeStructuralLines 前置空格解决)、
 * 首行开启却未闭合的围栏(由 closeDanglingFence 补闭合行——首行是本块正文,加不了
 * 空格)。谁给 parseOutline 教了第四种结构形状,必须在同一个 commit 里教给这里。
 *
 * 标题前缀刻意排在两步围栏处理*之前*:加了 `#` 之后首行不再以反引号开头,也就不再
 * 开启围栏——而 parseOutline 读回时同样如此。先加前缀,才能让转义的 raw 区跟踪与
 * closeDanglingFence 跟解析器(以及彼此)保持一致。与 Rust 侧 convert.rs 同序。
 */
function blockContent(b: RoamBlock, renames: Map<string, string>): string {
  let s = normalizeDateLinks(rewriteLinks(convertInline(b.string ?? ''), renames))
  if (b.heading != null && b.heading >= 1 && b.heading <= 3) s = `${'#'.repeat(b.heading)} ${s}`
  return closeDanglingFence(escapeStructuralLines(s))
}

/** RoamPage → 完整 .note.md 文本。renames 驱动全图重链。 */
export function convertPage(page: RoamPage, renames: Map<string, string>): ConvertedPage {
  // Daily notes must match note.md's native convention (src/lib/outline/daily.ts):
  // front-matter title is the `yyyy-MM-dd` date string itself, not Roam's English
  // "August 15th, 2022". Non-daily pages keep their original human title.
  const dailyDate = dailyDateFromUid(page.uid)
  const displayTitle = dailyDate ?? page.title
  const tree = createTree()
  tree.frontmatter = touchFrontmatter(null, {
    title: displayTitle,
    // 日记页与普通页的 OKF 类型不同(取值登记在宿主 src/lib/okf/concept.ts)
    type: dailyDate ? 'Daily Note' : 'Outline Note',
    created: iso(page['create-time']),
    now: iso(page['edit-time']) ?? new Date().toISOString(),
  })
  const walk = (bs: RoamBlock[] | undefined, parentId: string | null) => {
    ;(bs ?? []).forEach((b, idx) => {
      const node: OutlineNode = {
        id: b.uid ?? newId(),
        parentId,
        order: idx * 100,
        content: blockContent(b, renames),
        collapsed: false,
        source: 'manual',
        // 每个有 uid 的 block 都写 id::,与 Rust 侧 backend/src/convert.rs 一致:
        // CLI 同步的合并按 id:: 对齐 Roam 块与本地块。少了它,后续同步认不出任何
        // 旧块,整页会被当成「全是新块」再写一遍——页面翻倍。只写 ((ref)) 目标的
        // 旧规则正是这个数据 bug 的源头。无 uid 的块 id 是每次现生成的 UUID,写下去
        // 只会每次导入造一个新身份,故不写。
        persistId: b.uid != null ? true : undefined,
        createdAt: iso(b['create-time']),
        updatedAt: iso(b['edit-time']),
      }
      addNode(tree, node)
      walk(b.children, node.id)
    })
  }
  walk(page.children, null)
  if (tree.nodes.size === 0) {
    addNode(tree, { id: newId(), parentId: null, order: 0, content: '', collapsed: false, source: 'manual' })
  }
  return { title: displayTitle, text: serializeOutline(tree), editTime: maxEditTime(page) }
}
