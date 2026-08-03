#!/usr/bin/env node
// OKF v0.2 硬约束校验器 —— 只报告,不修改。
//
//   node scripts/okf-lint.mjs <目录> [--json] [--quiet]
//
// 递归扫描目录下的 `.md`,按 docs/okf-v0.2-format-constraints.md §11 的三条
// 硬约束报告违反项;退出码 1 表示存在违反。规则实现见 okf-lint-core.mjs。
//
// 目录根的 index.md 按 §8 允许携带只含 okf_version 的 frontmatter,故扫描根
// 目录被当作 bundle 根。
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { lintText } from './okf-lint-core.mjs'

const SKIP_DIRS = new Set(['.git', 'node_modules', '.notemd', 'dist', 'target'])

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith('.') && entry.name !== '.') {
      if (SKIP_DIRS.has(entry.name)) continue
    }
    const p = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue
      walk(p, out)
    } else if (entry.name.endsWith('.md')) {
      out.push(p)
    }
  }
  return out
}

const args = process.argv.slice(2)
const root = args.find((a) => !a.startsWith('--'))
const asJson = args.includes('--json')
const quiet = args.includes('--quiet')

if (!root) {
  console.error('usage: node scripts/okf-lint.mjs <目录> [--json] [--quiet]')
  process.exit(2)
}
if (!statSync(root).isDirectory()) {
  console.error(`not a directory: ${root}`)
  process.exit(2)
}

const violations = []
let scanned = 0
for (const file of walk(root)) {
  scanned++
  const rel = relative(root, file)
  const bundleRoot = !rel.includes('/')
  violations.push(...lintText(rel, readFileSync(file, 'utf8'), { bundleRoot }))
}

if (asJson) {
  console.log(JSON.stringify({ scanned, violations }, null, 2))
} else {
  for (const v of violations) console.log(`${v.file}: [${v.rule}] ${v.message}`)
  if (!quiet) {
    console.log(
      violations.length === 0
        ? `OKF: ${scanned} 份文档全部满足硬约束`
        : `OKF: ${scanned} 份文档中 ${violations.length} 处违反硬约束`,
    )
  }
}

process.exit(violations.length === 0 ? 0 : 1)
