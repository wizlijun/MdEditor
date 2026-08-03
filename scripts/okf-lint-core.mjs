// OKF v0.2 硬约束校验核心(docs/okf-v0.2-format-constraints.md §11)。
//
// 只报告、不修改,且只查规范里真正的硬约束三条:
//   1. 每个非保留 `.md` 含可解析的 YAML frontmatter;
//   2. 每个 frontmatter 含非空 `type`;
//   3. 保留文件名 `index.md` / `log.md` 不得用作概念文档(§8/§9)。
// 其余字段族全是 RECOMMENDED/MAY,缺失不算违反(§11 宽容一致性)。
//
// 纯 JS:CLI(scripts/okf-lint.mjs)与 vitest 共用同一份实现
// (仓库既有惯例,见 scripts/insights-report-core.mjs)。
import YAML from 'yaml'

/** §8/§9 的保留文件名。 */
export const RESERVED = ['index.md', 'log.md']

const FM_RE = /^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/

const violation = (file, rule, message) => ({ file, rule, message })

/**
 * 校验一份 markdown 文本。`opts.bundleRoot` 为 true 时,`index.md` 允许携带
 * 只含 `okf_version` 一个键的 frontmatter(§8 里唯一允许 frontmatter 的位置)。
 */
export function lintText(file, text, opts = {}) {
  const base = file.split('/').pop()
  const m = text.match(FM_RE)

  if (RESERVED.includes(base)) {
    if (!m) return []
    if (base === 'index.md' && opts.bundleRoot) {
      const doc = tryParse(m[1])
      if (doc === PARSE_ERROR) return [violation(file, 'frontmatter-unparsable', 'frontmatter 不是可解析的 YAML(§11 条件 1)')]
      const keys = isMapping(doc) ? Object.keys(doc) : []
      if (keys.length === 1 && keys[0] === 'okf_version') return []
      return [violation(file, 'index-extra-keys', 'bundle 根 index.md 的 frontmatter 只允许 okf_version 一个键(§8)')]
    }
    return [violation(file, 'reserved-as-concept', `${base} 是保留文件名,MUST NOT 用作概念文档(§8/§9)`)]
  }

  if (!m) return [violation(file, 'frontmatter-missing', '缺少 YAML frontmatter(§11 条件 1)')]
  const doc = tryParse(m[1])
  if (doc === PARSE_ERROR || !isMapping(doc)) {
    return [violation(file, 'frontmatter-unparsable', 'frontmatter 不是可解析的 YAML mapping(§11 条件 1)')]
  }
  const type = doc.type
  if (typeof type !== 'string' || type.trim() === '') {
    return [violation(file, 'type-missing', 'frontmatter 缺少非空 type 字段(§4.1 REQUIRED / §11 条件 2)')]
  }
  return []
}

const PARSE_ERROR = Symbol('parse-error')

function tryParse(raw) {
  try {
    return YAML.parse(raw)
  } catch {
    return PARSE_ERROR
  }
}

function isMapping(v) {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}
