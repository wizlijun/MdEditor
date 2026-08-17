#!/usr/bin/env node
// CHANGELOG 的发版侧工具。逻辑在 changelog-core.mjs(有单测),这里只做 IO。
//
//   node scripts/changelog.mjs check                 # 门禁:两份都写了吗、有没有漂移
//   node scripts/changelog.mjs rotate 6.817.1 2026-08-17
//   node scripts/changelog.mjs notes 6.817.1         # 打印英文版该节正文
//
// 由 scripts/release.sh 调用。设计见 docs/superpowers/specs/
// 2026-08-17-changelog-gate-design.md。

import { readFileSync, writeFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

import { checkGate, rotate, sectionFor, ChangelogError } from './changelog-core.mjs'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const EN = join(ROOT, 'CHANGELOG.md')
const ZH = join(ROOT, 'CHANGELOG.zh-CN.md')

const read = (p) => readFileSync(p, 'utf8')
const die = (msg) => {
  process.stderr.write(`${msg}\n`)
  process.exit(1)
}

const [cmd, ...rest] = process.argv.slice(2)

try {
  if (cmd === 'check') {
    const problems = checkGate(read(EN), read(ZH))
    if (problems.length) die(problems.map((p) => `  ✗ ${p}`).join('\n'))
    process.stdout.write('changelog ok\n')
  } else if (cmd === 'rotate') {
    const [version, date] = rest
    if (!version || !date) die('usage: changelog.mjs rotate <version> <YYYY-MM-DD>')
    writeFileSync(EN, rotate(read(EN), 'en', version, date))
    writeFileSync(ZH, rotate(read(ZH), 'zh', version, date))
    process.stdout.write(`changelog rotated to v${version}\n`)
  } else if (cmd === 'notes') {
    const [version] = rest
    if (!version) die('usage: changelog.mjs notes <version>')
    process.stdout.write(sectionFor(read(EN), 'en', version))
  } else {
    die('usage: changelog.mjs check | rotate <version> <date> | notes <version>')
  }
} catch (e) {
  if (e instanceof ChangelogError) die(`  ✗ ${e.message}`)
  throw e
}
