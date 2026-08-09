#!/usr/bin/env node
// 把一个平台条目并进已存在的 updater manifest,写回原文件。
//
//   node scripts/merge-latest-json.mjs \
//     --file latest.json \
//     --platform windows-x86_64 \
//     --url https://github.com/wizlijun/note.md/releases/download/v6.808.3/note.md_6.808.3_x64-setup.nsis.zip \
//     --sig-file path/to/note.md_6.808.3_x64-setup.nsis.zip.sig \
//     [--version 6.808.3]
//
// 逻辑在 merge-latest-json-core.mjs(有单测)。这里只做 IO 与参数解析。
// 全流程见 docs/windows-agent-brief.md。

import { readFileSync, writeFileSync } from 'node:fs'
import { mergePlatform, describe, MergeError } from './merge-latest-json-core.mjs'

function arg(name) {
  const i = process.argv.indexOf(`--${name}`)
  return i >= 0 ? process.argv[i + 1] : undefined
}

const file = arg('file')
const platform = arg('platform')
const url = arg('url')
const sigFile = arg('sig-file')
const version = arg('version')

if (!file || !platform || !url || !sigFile) {
  console.error(
    'usage: merge-latest-json.mjs --file <latest.json> --platform <key> --url <url> --sig-file <path> [--version <x.y.z>]',
  )
  process.exit(2)
}

try {
  const manifest = JSON.parse(readFileSync(file, 'utf8'))
  const signature = readFileSync(sigFile, 'utf8').trim()
  const before = describe(manifest)
  const merged = mergePlatform(manifest, { platform, url, signature, version })
  writeFileSync(file, JSON.stringify(merged, null, 2) + '\n')
  console.log(`merged ${platform} into ${file}`)
  console.log(`  platforms before: ${before}`)
  console.log(`  platforms after:  ${describe(merged)}`)
} catch (e) {
  if (e instanceof MergeError) {
    console.error(`✗ ${e.message}`)
    process.exit(1)
  }
  throw e
}
