// src/lib/outline/markdown.ts
import { createTree, addNode, childrenOf, newId, type OutlineTree, type OutlineNode, type NodeSource } from './model'

const PROP_RE = /^(type|line|id|collapsed|created|updated|status|answered|by):: (.*)$/

/** 文件头部 YAML front-matter 块。必须从第 0 字符开始,--- 独占一行。 */
const FM_RE = /^---\r?\n(?:([\s\S]*?)\r?\n)?---(\r?\n|$)/

export function splitFrontmatterBlock(text: string): { frontmatter: string | null; body: string } {
  const m = text.match(FM_RE)
  return m ? { frontmatter: m[1] ?? '', body: text.slice(m[0].length) } : { frontmatter: null, body: text }
}

/**
 * `\r` 是行结束符噪音,不是内容。
 *
 * 一个 `.note.md` 会从外面回来:Windows 编辑器、`core.autocrlf` 检出、同步过来的
 * 文件——file-over-app 承诺扛住的正是这些流量。而 JS 的 `.` 和 `$` 都把 `\r` 当行
 * 终止符,所以以 `\r` 收尾的 bullet 行**匹配不上** bullet 正则,整行掉进兜底分支:
 * 子节点被父节点当续行吞掉、属性行变成可见正文——和空块丢掉行尾空格是同一种崩坏,
 * 又一个看不见的字节决定了节点存不存在。
 *
 * 更要命的是 Rust 移植版(plugins-src/roam-import/backend/src/outline.rs)用的
 * regex crate 里 `.` **会**匹配 `\r`,于是同一个 CRLF 文件在主程序和插件后端读出来
 * 是两棵不同的树。「一个 vault,多个 agent」的前提下这不可接受:两侧一致才是要求本身。
 *
 * 所以在解析入口一次性去掉所有 `\r`,而不是往每条正则里撒 `\r?`——一个地方,两侧
 * 逐字相同,不必去推 JS 与 Rust 的正则方言差异,后来的人也没有地方可漏。
 * 代价明说:CRLF 文件读进来再写回去会变成 LF(序列化端一个字节没动)。
 */
const stripCarriageReturns = (text: string): string =>
  text.includes('\r') ? text.replace(/\r/g, '') : text

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
  const { frontmatter, body } = splitFrontmatterBlock(stripCarriageReturns(text))
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
    // `-(?: (.*))?$`:空块被序列化成 `- `——破折号 + 空格 + 空内容,于是行尾空格承载了语义。
    // 编辑器/格式化器/git 钩子例行删行尾空白,而 file-over-app 把「vault 被外部改过」当常态。
    // 因此「只有缩进和一个 `-`」的行同样是空 bullet。写入端一个字节不改:修解析即修全部存量文件。
    // 必须是「`-` 后紧跟行尾或一个空格」,不能写成 `- ?`——否则 `--`/`---`(front-matter 围栏、
    // 分隔线)会被误认成 bullet。front-matter 在 bullet 扫描之前就已被 splitFrontmatterBlock 切走。
    const bullet = raw.match(/^((?:  )*)-(?: (.*))?$/)
    if (bullet) {
      const content = bullet[2] ?? ''
      current = push(bullet[1].length / 2, content)
      currentDepth = bullet[1].length / 2
      const open = content.match(/^(`{3,})/)
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
            // 冲突守卫：value 已被另一节点占用时，Map.set 会把那个节点从树里逐出
            // （childrenOf 只按 map 遍历），其整棵子树随下一次 serialize 静默消失。
            // vault 文件手改/多 agent 写入/git 合并都可能产生重复 id::，不是特例。
            // 因此:仅当 value 未被占用、或恰是本节点自己已持有的 id 时才重键;
            // 否则保留生成 id、不设 persistId——这条不可用的 id:: 行就不会被写回。
            const holder = tree.nodes.get(value)
            if (!holder || holder === current) {
              tree.nodes.delete(current.id)
              current.id = value
              tree.nodes.set(value, current)
              // Mark this id as explicitly set so it gets written back
              current.persistId = true
            } else {
              // 冲突不是无声的:一个 ((id)) 块引用若指向这个撞车的 id,现在会解析到
              // 先到者、而非本节点——内容不再丢失,但链接目标可能悄悄换了对象。
              // 这属于「人可能需要事后解释」的数据完整性事件,须留痕(单条 warn,每次撞车一条)。
              console.warn(`parseOutline: duplicate id:: "${value}" — keeping first holder, this node falls back to a generated id`)
            }
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
