// src/lib/roam-import/syntax.ts
/** 代码段(``` fence 或 `inline`)切分:偶数下标是普通文本,奇数是代码,转换只作用于普通段 */
const CODE_SPLIT_RE = /(```[\s\S]*?```|`[^`\n]*`)/

function mapNonCode(s: string, fn: (seg: string) => string): string {
  return s.split(CODE_SPLIT_RE).map((seg, i) => (i % 2 === 0 ? fn(seg) : seg)).join('')
}

/** Roam 行内语法 → 本地 markdown(spec 语法映射表) */
export function convertInline(s: string): string {
  return mapNonCode(s, (seg) =>
    seg
      .replace(/\{\{\[\[embed\]\]:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}/g, '(($1))')
      .replace(/\{\{embed:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}/g, '(($1))')
      .replace(/\{\{\[\[TODO\]\]\}\}/g, '[ ]')
      .replace(/\{\{\[\[DONE\]\]\}\}/g, '[x]')
      .replace(/\{\{TODO\}\}/g, '[ ]')
      .replace(/\{\{DONE\}\}/g, '[x]')
      .replace(/__([^_\n](?:[^\n]*?[^_\n])?)__/g, '*$1*')
      .replace(/#\[\[([^\]\n]+)\]\]/g, '[[$1]]'),
  )
}

const MONTHS: Record<string, string> = {
  january: '01', february: '02', march: '03', april: '04', may: '05', june: '06',
  july: '07', august: '08', september: '09', october: '10', november: '11', december: '12',
}
/** Roam 日记标题写法 "August 15th, 2022" → "2022-08-15";非该形式返回 null。 */
export function toIsoDate(target: string): string | null {
  const m = target.match(/^([A-Za-z]+) (\d{1,2})(?:st|nd|rd|th), (\d{4})$/)
  if (!m) return null
  const mo = MONTHS[m[1].toLowerCase()]
  const dd = Number(m[2])
  if (!mo || dd < 1 || dd > 31) return null
  return `${m[3]}-${mo}-${String(dd).padStart(2, '0')}`
}

/** 把英文日期形式的 [[链接]] 规范成 [[yyyy-MM-dd]](note.md 只识别 ISO 日期链接,
 *  spec §6)。不依赖导出里是否存在对应日记页,故空白日期链接也能正确指向。 */
export function normalizeDateLinks(s: string): string {
  return mapNonCode(s, (seg) =>
    seg.replace(/\[\[([^\]\n]+)\]\]/g, (whole, t: string) => {
      const iso = toIsoDate(t)
      return iso != null ? `[[${iso}]]` : whole
    }),
  )
}

/** 按改名映射改写 [[链接]](wikilink 只按文件名解析,改名必须全图重链) */
export function rewriteLinks(s: string, renames: Map<string, string>): string {
  if (renames.size === 0) return s
  return mapNonCode(s, (seg) =>
    seg.replace(/\[\[([^\]\n]+)\]\]/g, (whole, t: string) => {
      const to = renames.get(t)
      return to != null ? `[[${to}]]` : whole
    }),
  )
}

/** parseOutline 的 PROP_RE(宿主 src/lib/outline/markdown.ts):九个保留键。
 *  少一个键就是一个转义漏洞——该续行会被当属性吃掉。 */
const RESERVED_PROP_RE = /^(type|line|id|collapsed|created|updated|status|answered|by):: /

/** parseOutline 认的 bullet 形状(`^((?:  )*)- `):偶数个前导空格 + `- `。 */
const BULLET_LINE_RE = /^(?:  )*- /

/** 围栏开启/闭合判定,与 parseOutline 的内联正则同源(bullet 首行 `^(`{3,})`
 *  开启 raw 模式,续行 `^(`{3,})\s*$` 且不短于开启长度才闭合)。Rust 侧把同一对
 *  规则提成了 outline.rs 的 fence_open_len / fence_close_len。 */
function fenceOpenLen(s: string): number | null {
  const m = s.match(/^(`{3,})/)
  return m ? m[1].length : null
}
function fenceCloseLen(s: string): number | null {
  const m = s.match(/^(`{3,})\s*$/)
  return m ? m[1].length : null
}

/**
 * 中和多行 block 里会被 parseOutline 读成「结构」而非「本块正文」的续行:
 *
 * * `key:: value` —— 节点*属性*行,会被从正文里摘走;若是 `id::` 更会改写本块身份。
 * * `  - text` —— *子 bullet*。Roam 里 shift-enter 打的清单
 *   (`shopping\n- milk\n- eggs`)正是这个形状。
 *
 * 两者都用前置一个空格修:渲染等价,且该行不再匹配。对 bullet 这是机械保证——
 * `^((?:  )*)- ` 要求偶数个前导空格,加一个变奇数,任何深度都不再匹配;加空格也
 * 永远不会造出新的匹配行,这就是幂等的来源。
 *
 * **围栏感知,因为转义绝不能改用户贴进来的代码。** 当本块*首行*开启围栏时,
 * parseOutline 进入 raw 模式,逐行原样收到不短于开启长度的闭合行为止——那段里没有
 * 任何东西会被误读成结构,故那段里也不许动一个字符。否则一段 fenced YAML 里的
 * `- foo` 会被同步悄悄插进一个空格。闭合之后的行重新按结构解析,故重新转义。
 *
 * 而在*后续行*才开启的围栏不是 raw 模式(parseOutline 只从 bullet 首行进入),
 * 那些行仍要转义,哪怕看起来像代码:不转义的代价是本块丢掉 `id::` 锚点、被合并
 * 当新块反复重建。往返保真优先于围栏观感;首行情形(常见情形,也是 Roam 自己的
 * code block 产出的情形)是精确的。
 *
 * 与 Rust 侧 backend/src/syntax.rs 的 escape_structural_lines 逐字对应。
 */
export function escapeStructuralLines(s: string): string {
  // >0 = 处在首行开启的 raw 围栏内,与 parseOutline 用同一对判定跟踪
  let fence = 0
  const out: string[] = []
  s.split('\n').forEach((ln, i) => {
    if (i === 0) {
      // 首行就是 bullet 自己的正文(写在 `- ` 之后),永远不是结构
      fence = fenceOpenLen(ln) ?? 0
      out.push(ln)
      return
    }
    if (fence > 0) {
      const close = fenceCloseLen(ln)
      if (close != null && close >= fence) fence = 0
      out.push(ln)
      return
    }
    out.push(RESERVED_PROP_RE.test(ln) || BULLET_LINE_RE.test(ln) ? ` ${ln}` : ln)
  })
  return out.join('\n')
}

/** 首行开启却从未闭合的围栏:parseOutline 会一路 raw 吃到后面几个块里去(闭合行
 *  在别的块上),下次读回时后续块整片消失。把闭合行补给它自己。
 *  与 Rust 侧 backend/src/convert.rs 的 close_dangling_fence 对应。 */
export function closeDanglingFence(s: string): string {
  const lines = s.split('\n')
  const open = fenceOpenLen(lines[0] ?? '')
  if (open == null) return s
  // 只有开启行*之后*的行能闭合它(parseOutline 只检查续行)
  for (const ln of lines.slice(1)) {
    const close = fenceCloseLen(ln)
    if (close != null && close >= open) return s
  }
  return `${s}\n${'`'.repeat(open)}`
}
