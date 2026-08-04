// OKF v0.2 bundle 导出的纯逻辑(CLI 在 scripts/okf-export.mjs)。
//
// vault 里用的是 `[[wikilink]]` 与 `((block-ref))` —— Obsidian 生态的刻意选择;
// 但通用 OKF 消费者只认标准 Markdown 链接(§6),所以**导出时**才转换,vault 本身
// 不动。同理:导出副本一定带上 §4.1 必填的 `type`,而磁盘上的存量文件不被改写。
import YAML from 'yaml'

const FM_RE = /^---\r?\n(?:([\s\S]*?)\r?\n)?---(\r?\n|$)/
/** 行内代码/围栏里的 `[[...]]` 是内容,不是链接。 */
const CODE_RE = /(`{1,3})[\s\S]*?\1/g
const WIKILINK_RE = /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g

/**
 * `[[Target]]` / `[[Target|alias]]` → `[alias](/bundle/relative/path.md)`。
 * `index` 是「wikilink 目标 → bundle 内相对路径」的映射;解析不到的链接降级成
 * 纯文本(§6 允许断链,但一条指向空处的链接不如直接把话说完)。
 */
export function rewriteLinks(md, index) {
  const spans = []
  for (const m of md.matchAll(CODE_RE)) spans.push([m.index, m.index + m[0].length])
  const inCode = (i) => spans.some(([a, b]) => i >= a && i < b)

  return md.replace(WIKILINK_RE, (whole, target, alias, offset) => {
    if (inCode(offset)) return whole
    const rel = index.get(target.trim())
    const text = (alias ?? target).trim()
    return rel ? `[${text}](/${rel})` : text
  })
}

/** 文档首部的 frontmatter 原文(没有则 null)。 */
export function frontmatterOf(md) {
  const m = md.match(FM_RE)
  return m ? (m[1] ?? '') : null
}

function bodyOf(md) {
  const m = md.match(FM_RE)
  return m ? md.slice(m[0].length) : md
}

/** 首个 ATX H1(frontmatter 不参与)。 */
export function titleOf(md) {
  const m = bodyOf(md).match(/^#\s+(.+?)\s*$/m)
  return m ? m[1] : null
}

/** frontmatter 的 description(没有则空串)。 */
export function descriptionOf(md) {
  const fm = frontmatterOf(md)
  if (fm == null) return ''
  try {
    const v = YAML.parse(fm)
    return v && typeof v === 'object' && typeof v.description === 'string' ? v.description : ''
  } catch {
    return ''
  }
}

/** 路径 → 该文档的 OKF type(与 src/lib/okf/concept.ts 的登记表一致)。 */
function typeForPath(rel) {
  if (/\.notes?\.md$/i.test(rel)) {
    if (rel.includes('dailynote/')) return 'Daily Note'
    if (rel.includes('wikipage/')) return 'Wiki Page'
    return 'Outline Note'
  }
  return 'Note'
}

/**
 * 导出副本的 frontmatter:缺 `type` 就按路径补一个,已有的一律不动
 * (§4.1 的往返要求:未知键与既有值原样保留)。
 */
export function stampConcept(md, rel) {
  const fm = frontmatterOf(md)
  const doc = YAML.parseDocument(fm ?? '')
  if (doc.contents == null) doc.contents = doc.createNode({})
  else if (doc.contents.constructor?.name !== 'YAMLMap') return md
  if (doc.has('type')) return md
  const fresh = YAML.parseDocument('')
  fresh.contents = fresh.createNode({})
  fresh.set('type', typeForPath(rel))
  const title = titleOf(md)
  if (title) fresh.set('title', title)
  for (const pair of doc.contents.items) fresh.set(pair.key.value ?? pair.key, pair.value)
  return `---\n${fresh.toString().replace(/\n$/, '')}\n---\n${bodyOf(md)}`
}

/** bundle 根 index.md 的 frontmatter:§8 只允许 okf_version 一个键。 */
export function bundleIndexHead() {
  return '---\nokf_version: "0.2"\n---\n'
}

/** §8 的目录索引:按目录分组,列表项带上被链概念的 description。 */
export function buildIndex(entries) {
  const groups = new Map()
  for (const e of entries) {
    const dir = e.rel.includes('/') ? e.rel.slice(0, e.rel.lastIndexOf('/')) : ''
    if (!groups.has(dir)) groups.set(dir, [])
    groups.get(dir).push(e)
  }
  const lines = []
  for (const dir of [...groups.keys()].sort()) {
    lines.push(`# ${dir === '' ? 'Root' : dir}`, '')
    for (const e of groups.get(dir).sort((a, b) => a.rel.localeCompare(b.rel))) {
      lines.push(`* [${e.title}](${e.rel})${e.description ? ` - ${e.description}` : ''}`)
    }
    lines.push('')
  }
  return lines.join('\n')
}

/** §9 的变更日志:按 ISO 日期分组,最新在前。 */
export function buildLog(commits) {
  const byDate = new Map()
  for (const c of commits) {
    if (!byDate.has(c.date)) byDate.set(c.date, [])
    byDate.get(c.date).push(c.subject)
  }
  const lines = ['# Directory Update Log', '']
  for (const date of [...byDate.keys()].sort().reverse()) {
    lines.push(`## ${date}`, '')
    for (const subject of byDate.get(date)) lines.push(`* ${subject}`)
    lines.push('')
  }
  return lines.join('\n')
}
