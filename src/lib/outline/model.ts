// src/lib/outline/model.ts
export type NodeSource = 'toc' | 'highlight' | 'wikilink' | 'annotation' | 'note' | 'question' | 'answer' | 'manual'
export type QuestionStatus = 'open' | 'answered' | 'closed' | 'adopted'

export interface OutlineNode {
  id: string
  parentId: string | null // null = 根层
  order: number           // 同级分数排序（hulunote same-deep-order）
  content: string
  collapsed: boolean
  source: NodeSource
  anchorLine?: number     // auto 节点：主文档 1-based 行号
  /** id:: was explicitly present in the companion file (or must be written); survives node copies */
  persistId?: boolean
  /** ISO 8601 创建时间；仅 highlight/manual 节点记录，toc 不记 */
  createdAt?: string
  /** ISO 8601 最近内容修改时间；仅 highlight/manual 节点记录 */
  updatedAt?: string
  /** 仅 source==='question':问答状态机(spec 2026-07-27-annotation-qa-loop)。缺省视为 open */
  status?: QuestionStatus
  /** ✦ 作答节点:作答时间(ISO8601),由外部 agent 写入,须原样 roundtrip */
  answeredAt?: string
  /** ✦ 作答节点:作答者标识(如 claude-code),由外部 agent 写入,须原样 roundtrip */
  answeredBy?: string
}

export function nowIso(): string {
  return new Date().toISOString()
}

/** 统一的内容修改入口：内容变化且非 toc 节点时刷新 updatedAt */
export function setNodeContent(node: OutlineNode, content: string): void {
  if (node.content === content) return
  node.content = content
  if (node.source !== 'toc') node.updatedAt = nowIso()
}

export interface OutlineTree { nodes: Map<string, OutlineNode>; frontmatter: string | null }

export function createTree(): OutlineTree { return { nodes: new Map(), frontmatter: null } }

export function addNode(tree: OutlineTree, node: OutlineNode): void {
  tree.nodes.set(node.id, node)
}

export function childrenOf(tree: OutlineTree, parentId: string | null): OutlineNode[] {
  const out: OutlineNode[] = []
  for (const n of tree.nodes.values()) if (n.parentId === parentId) out.push(n)
  return out.sort((a, b) => a.order - b.order)
}

/** hulunote render.cljs:612 calculate-order-between */
export function calculateOrderBetween(prev: number | null, next: number | null): number {
  if (prev != null && next != null) return (prev + next) / 2
  if (prev != null) return prev + 100
  if (next != null) return next / 2
  return 0
}

/** hulunote render.cljs:591 normalize-sibling-orders! — idx*100 */
export function normalizeSiblingOrders(tree: OutlineTree, parentId: string | null): void {
  childrenOf(tree, parentId).forEach((n, idx) => { n.order = idx * 100 })
}

/** 从 id 沿 parentId 上溯到根,返回祖先链(根在前、直接父在后;**不含 id 本身**)。
 *  用于 zoom 面包屑(hulunote get-nav-breadcrumbs + butlast)。带环保护:遇到已访问
 *  的 id 立即停止,防坏树死循环。id 不存在 → 空数组。 */
export function ancestorsOf(tree: OutlineTree, id: string): OutlineNode[] {
  const chain: OutlineNode[] = []
  const seen = new Set<string>([id])
  let pid = tree.nodes.get(id)?.parentId ?? null
  while (pid != null && !seen.has(pid)) {
    const p = tree.nodes.get(pid)
    if (!p) break
    seen.add(pid)
    chain.push(p)
    pid = p.parentId
  }
  return chain.reverse()
}

/** hulunote render.cljs:639 collect-descendant-ids */
export function collectDescendantIds(tree: OutlineTree, id: string): Set<string> {
  const acc = new Set<string>()
  const walk = (pid: string) => {
    for (const c of childrenOf(tree, pid)) { acc.add(c.id); walk(c.id) }
  }
  walk(id)
  return acc
}

/** hulunote render.cljs:663 valid-drop-target? */
export function isValidDropTarget(tree: OutlineTree, dragId: string, targetId: string): boolean {
  return !!dragId && !!targetId && dragId !== targetId
    && !collectDescendantIds(tree, dragId).has(targetId)
}

/** hulunote render.cljs:748 collect-visible-navs — 折叠节点不展开其子树 */
export function visibleNodes(tree: OutlineTree): OutlineNode[] {
  const out: OutlineNode[] = []
  const walk = (pid: string | null) => {
    for (const n of childrenOf(tree, pid)) {
      out.push(n)
      if (!n.collapsed) walk(n.id)
    }
  }
  walk(null)
  return out
}

export function removeSubtree(tree: OutlineTree, id: string): void {
  for (const d of collectDescendantIds(tree, id)) tree.nodes.delete(d)
  tree.nodes.delete(id)
}

export function newId(): string {
  return crypto.randomUUID()
}

/** 批注文本含半角/全角问号即视为「向 agent 提问」(spec:自然书写即协议) */
export function isQuestionText(s: string): boolean {
  return /[?？]/.test(s)
}

/** 树中存在任何 question 节点(用于:含提问即激活伴生笔记落盘) */
export function treeHasQuestion(tree: OutlineTree): boolean {
  for (const n of tree.nodes.values()) if (n.source === 'question') return true
  return false
}

/** 开/闭代码围栏行(≥3 反引号)。答复正文整体被围栏包住,使任意 markdown
 *  (列表、`key::` 样式的行、嵌套代码块)对大纲解析器完全不透明。 */
const FENCE_OPEN_RE = /^(`{3,})/
const FENCE_CLOSE_RE = /^(`{3,})\s*$/

/**
 * 把答复正文包进自定界围栏。围栏长度 = 正文内最长反引号串 + 1(不少于 3),
 * 依 CommonMark 规则保证嵌套代码块不会提前闭合。
 */
export function wrapAnswerBody(body: string): string {
  let longest = 0
  for (const m of body.matchAll(/`+/g)) longest = Math.max(longest, m[0].length)
  const fence = '`'.repeat(Math.max(3, longest + 1))
  return `${fence}markdown\n${body}\n${fence}`
}

/** 取答复节点的正文(剥掉首尾围栏行)。无围栏时原样返回(fail open)。 */
export function answerBodyOf(node: Pick<OutlineNode, 'content'>): string {
  const lines = node.content.split('\n')
  const open = lines[0]?.match(FENCE_OPEN_RE)
  if (!open) return node.content
  const last = lines.length - 1
  const close = lines[last]?.match(FENCE_CLOSE_RE)
  const end = close && close[1].length >= open[1].length ? last : lines.length
  return lines.slice(1, end).join('\n')
}

/** 树中该 question 节点下的答复节点(至多一个) */
export function answerNodeOf(tree: OutlineTree, questionId: string): OutlineNode | undefined {
  return childrenOf(tree, questionId).find(c => c.source === 'answer')
}
