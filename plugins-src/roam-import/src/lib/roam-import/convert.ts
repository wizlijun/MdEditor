// src/lib/roam-import/convert.ts
import { createTree, addNode, newId, type OutlineNode } from '../outline/model'
import { serializeOutline } from '../outline/markdown'
import { touchFrontmatter } from '../outline/frontmatter'
import { convertInline, rewriteLinks, normalizeDateLinks, escapeReservedProps } from './syntax'
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

function blockContent(b: RoamBlock, renames: Map<string, string>): string {
  let s = escapeReservedProps(normalizeDateLinks(rewriteLinks(convertInline(b.string ?? ''), renames)))
  if (b.heading != null && b.heading >= 1 && b.heading <= 3) s = `${'#'.repeat(b.heading)} ${s}`
  return s
}

/** RoamPage → 完整 .note.md 文本。refUids 决定哪些节点写 id::,renames 驱动全图重链。 */
export function convertPage(page: RoamPage, refUids: Set<string>, renames: Map<string, string>): ConvertedPage {
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
        persistId: b.uid != null && refUids.has(b.uid) ? true : undefined,
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
