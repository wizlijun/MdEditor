#!/usr/bin/env node
// 把 vault(或它的一个子树)导出成一个合规的 OKF v0.2 Knowledge Bundle。
//
//   node scripts/okf-export.mjs <源目录> <目标目录> [--ignore <glob>]... [--quiet]
//
// 做四件事(逻辑在 okf-export-core.mjs,可单测):
//   1. 复制每份 `.md`,缺 `type` 的按路径补上(§4.1)——**只改副本,不动源**;
//   2. `[[wikilink]]` → bundle 绝对路径的 Markdown 链接(§6),解析不到的降级成纯文本;
//   3. 生成根 `index.md`(带 `okf_version: "0.2"`,§8)与 `log.md`(git 历史,§9);
//   4. 导出后自查:整包过 `okf-lint-core` 的硬约束,不过就非零退出。
//
// 保留名(index.md / log.md)不进正文复制:它们由本工具生成。
import { readdirSync, readFileSync, writeFileSync, mkdirSync, statSync, copyFileSync } from 'node:fs'
import { join, relative, dirname } from 'node:path'
import { execFileSync } from 'node:child_process'
import { lintText, shouldIgnore, RESERVED } from './okf-lint-core.mjs'
import {
  rewriteLinks, buildIndex, buildLog, stampConcept, bundleIndexHead, titleOf, descriptionOf,
} from './okf-export-core.mjs'

const SKIP_DIRS = new Set(['.git', 'node_modules', '.notemd', 'dist', 'target'])

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (SKIP_DIRS.has(entry.name) || entry.name.startsWith('.')) continue
    const p = join(dir, entry.name)
    if (entry.isDirectory()) walk(p, out)
    else out.push(p)
  }
  return out
}

/** wikilink 目标 → bundle 相对路径。目标按文件名解析(与 vault 内一致)。 */
function linkIndex(files) {
  const index = new Map()
  for (const rel of files) {
    const base = rel.slice(rel.lastIndexOf('/') + 1).replace(/\.notes?\.md$/i, '').replace(/\.md$/i, '')
    if (!index.has(base)) index.set(base, rel)
  }
  return index
}

/** git 历史 → log.md 的条目;不是 git 仓库就返回空(§9 的 log.md 是可选的)。 */
function gitLog(dir) {
  try {
    const out = execFileSync('git', ['log', '--date=short', '--format=%ad\x1f%s', '-n', '200'], {
      cwd: dir, encoding: 'utf8',
    })
    return out.split('\n').filter(Boolean).map((line) => {
      const [date, subject] = line.split('\x1f')
      return { date, subject }
    })
  } catch {
    return []
  }
}

const args = process.argv.slice(2)
const positional = args.filter((a, i) => !a.startsWith('--') && args[i - 1] !== '--ignore')
const [src, dest] = positional
const ignore = args.flatMap((a, i) => (args[i - 1] === '--ignore' ? [a] : []))
const quiet = args.includes('--quiet')

if (!src || !dest) {
  console.error('usage: node scripts/okf-export.mjs <源目录> <目标目录> [--ignore <glob>]... [--quiet]')
  process.exit(2)
}
if (!statSync(src).isDirectory()) {
  console.error(`not a directory: ${src}`)
  process.exit(2)
}

const all = walk(src).map((p) => relative(src, p)).filter((rel) => !shouldIgnore(rel, ignore))
const mds = all.filter((rel) => rel.endsWith('.md') && !RESERVED.includes(rel.split('/').pop()))
const index = linkIndex(mds)

const entries = []
for (const rel of mds) {
  const raw = readFileSync(join(src, rel), 'utf8')
  const out = stampConcept(rewriteLinks(raw, index), rel)
  mkdirSync(dirname(join(dest, rel)), { recursive: true })
  writeFileSync(join(dest, rel), out)
  entries.push({ rel, title: titleOf(out) ?? rel, description: descriptionOf(out) })
}

// 非 markdown 的附件(图片等)原样带走,否则导出的包里图链全断。
for (const rel of all.filter((r) => !r.endsWith('.md'))) {
  mkdirSync(dirname(join(dest, rel)), { recursive: true })
  copyFileSync(join(src, rel), join(dest, rel))
}

writeFileSync(join(dest, 'index.md'), bundleIndexHead() + buildIndex(entries))
const commits = gitLog(src)
if (commits.length > 0) writeFileSync(join(dest, 'log.md'), buildLog(commits))

// 自查:导出的包必须自己过硬约束,否则这个工具就是在生产不合规的包。
const violations = []
for (const rel of [...mds, 'index.md', ...(commits.length > 0 ? ['log.md'] : [])]) {
  violations.push(...lintText(rel, readFileSync(join(dest, rel), 'utf8'), { bundleRoot: !rel.includes('/') }))
}
for (const v of violations) console.error(`${v.file}: [${v.rule}] ${v.message}`)

if (!quiet) {
  console.log(
    violations.length === 0
      ? `OKF bundle: ${mds.length} 份概念 + index.md${commits.length ? ' + log.md' : ''} → ${dest}`
      : `OKF bundle: ${violations.length} 处违反硬约束(导出仍已写出,请检查)`,
  )
}
process.exit(violations.length === 0 ? 0 : 1)
