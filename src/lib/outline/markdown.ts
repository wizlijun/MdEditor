// src/lib/outline/markdown.ts
import { createTree, addNode, childrenOf, newId, type OutlineTree, type OutlineNode, type NodeSource } from './model'

const PROP_RE = /^(type|line|id|collapsed|created|updated|status|answered|by):: (.*)$/

/** 文件头部 YAML front-matter 块。必须从第 0 字符开始,--- 独占一行。 */
const FM_RE = /^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/

export function splitFrontmatterBlock(text: string): { frontmatter: string | null; body: string } {
  const m = text.match(FM_RE)
  return m ? { frontmatter: m[1], body: text.slice(m[0].length) } : { frontmatter: null, body: text }
}

/**
 * Serialize the tree to companion-file markdown.
 * `persistIds`: node ids that must be written (manual block-ref targets).
 * Nodes with `persistId === true` (set by parseOutline when `id::` was
 * explicitly present) are always written regardless of `persistIds`.
 */
/**
 * `viewOnlyCollapsed` holds ids collapsed as a VIEW default (answered questions
 * arrive folded). Their `collapsed::` is deliberately not written: opening a
 * note must not modify it.
 */
export function serializeOutline(
  tree: OutlineTree,
  persistIds: Set<string> = new Set(),
  omitCollapsed = false,
  viewOnlyCollapsed: ReadonlySet<string> = new Set(),
): string {
  const lines: string[] = []
  if (tree.frontmatter != null) lines.push('---', tree.frontmatter, '---')
  const walk = (parentId: string | null, depth: number) => {
    for (const n of childrenOf(tree, parentId)) {
      const indent = '  '.repeat(depth)
      const contentLines = n.content.split('\n')
      lines.push(`${indent}- ${contentLines[0]}`)
      // 空行写成空字符串(不写只含空白的行):答复正文里的段落空行必须双向稳定
      for (const cont of contentLines.slice(1)) lines.push(cont === '' ? '' : `${indent}  ${cont}`)
      if (n.source !== 'manual') {
        lines.push(`${indent}  type:: ${n.source}`)
        if (n.anchorLine != null) lines.push(`${indent}  line:: ${n.anchorLine}`)
        if (n.source === 'question') lines.push(`${indent}  status:: ${n.status ?? 'open'}`)
      }
      if (n.createdAt) lines.push(`${indent}  created:: ${n.createdAt}`)
      if (n.updatedAt) lines.push(`${indent}  updated:: ${n.updatedAt}`)
      if (n.answeredAt) lines.push(`${indent}  answered:: ${n.answeredAt}`)
      if (n.answeredBy) lines.push(`${indent}  by:: ${n.answeredBy}`)
      if (n.persistId === true || persistIds.has(n.id)) {
        lines.push(`${indent}  id:: ${n.id}`)
      }
      if (n.collapsed && !omitCollapsed && !viewOnlyCollapsed.has(n.id)) {
        lines.push(`${indent}  collapsed:: true`)
      }
      walk(n.id, depth + 1)
    }
  }
  walk(null, 0)
  return lines.length ? lines.join('\n') + '\n' : ''
}

export function parseOutline(text: string): OutlineTree {
  const tree = createTree()
  const { frontmatter, body } = splitFrontmatterBlock(text)
  tree.frontmatter = frontmatter

  // 每层的"当前节点"栈：stack[d] = 深度 d 的最近节点
  const stack: OutlineNode[] = []
  let current: OutlineNode | null = null
  let currentDepth = -1
  let orderCounters: number[] = []
  // >0 表示正处在答复围栏内(raw 模式):逐行原样收,不识别 bullet / 属性行
  let fenceLen = 0

  const nextOrder = (depth: number): number => {
    orderCounters.length = depth + 1
    orderCounters[depth] = (orderCounters[depth] ?? -100) + 100
    return orderCounters[depth]
  }

  const push = (depth: number, content: string): OutlineNode => {
    const parent = depth > 0 ? stack[depth - 1] ?? null : null
    const node: OutlineNode = {
      id: newId(),
      parentId: parent ? parent.id : null,
      order: nextOrder(depth),
      content,
      collapsed: false,
      source: 'manual',
    }
    addNode(tree, node)
    stack.length = depth
    stack[depth] = node
    return node
  }

  const lines = body.split('\n')
  // 文本以 \n 结尾时 split 必然多出一个尾部 '':那是结构产物,不是语义空行。
  // 常规路径本就跳过空行、不受影响;唯有围栏 raw 模式会把它当成答复正文的一部分
  // (未闭合围栏时尤其明显),故在此统一剔除一个。
  if (lines[lines.length - 1] === '') lines.pop()

  for (const raw of lines) {
    if (fenceLen > 0 && current) {
      // raw 模式:剥掉本节点的续行缩进(容忍手改文件:最多剥这么多前导空格),
      // 空行原样保留——markdown 段落间的空行是语义。
      const contIndent = '  '.repeat(currentDepth) + '  '
      const line = raw.startsWith(contIndent)
        ? raw.slice(contIndent.length)
        : raw.replace(new RegExp(`^ {0,${contIndent.length}}`), '')
      current.content += '\n' + line
      const close = line.match(/^(`{3,})\s*$/)
      if (close && close[1].length >= fenceLen) fenceLen = 0   // 闭合,回到常规解析
      continue
    }
    if (raw.trim() === '') continue
    const bullet = raw.match(/^((?:  )*)- (.*)$/)
    if (bullet) {
      current = push(bullet[1].length / 2, bullet[2])
      currentDepth = bullet[1].length / 2
      const open = bullet[2].match(/^(`{3,})/)
      if (open) fenceLen = open[1].length      // 进入 raw 模式
      continue
    }
    if (current) {
      // 续行或属性行：期望缩进 = 节点缩进 + 2
      const contIndent = '  '.repeat(currentDepth) + '  '
      if (raw.startsWith(contIndent)) {
        const body = raw.slice(contIndent.length)
        const prop = body.match(PROP_RE)
        if (prop) {
          const key = prop[1]
          // 属性值容错:外部工具/编辑器可能在属性行尾留空白(如 markdown 硬换行的两个空格)。
          // 去尾空白再解析,避免 `type:: question  ` / `status:: open  ` 因值不匹配白名单被静默丢弃
          // (file-over-app:vault 常被 Obsidian/格式化器/git-sync 改动,解析须稳健)。
          const value = prop[2].trimEnd()
          if (key === 'type' && ['toc', 'highlight', 'wikilink', 'annotation', 'note', 'question', 'answer'].includes(value)) current.source = value as NodeSource
          else if (key === 'line') current.anchorLine = parseInt(value, 10)
          else if (key === 'collapsed') current.collapsed = value === 'true'
          else if (key === 'created') current.createdAt = value
          else if (key === 'updated') current.updatedAt = value
          else if (key === 'status') {
            if (value === 'open' || value === 'answered' || value === 'closed' || value === 'adopted') {
              current.status = value
              // status:: 是 question 专属属性:即便 type:: 行缺失/损坏,合法 status 即认定为 question,
              // 保证 type/status 始终成对写回——自愈历史上 type 丢失只剩 status 的脏节点。
              if (current.source === 'manual' || current.source === 'note') current.source = 'question'
            }
          }
          else if (key === 'answered') current.answeredAt = value
          else if (key === 'by') current.answeredBy = value
          else if (key === 'id') {
            // 重键：换 id 需迁移 map（此时尚无子节点，直接迁移 map）
            // Invariant: id:: precedes any children of this node.
            tree.nodes.delete(current.id)
            current.id = value
            tree.nodes.set(value, current)
            // Mark this id as explicitly set so it gets written back
            current.persistId = true
          }
        } else {
          current.content += '\n' + body
        }
        continue
      }
    }
    // 无法归类的行：降级为根层手写节点（spec: 不丢内容）
    current = push(0, raw.trim())
    currentDepth = 0
  }
  return tree
}
