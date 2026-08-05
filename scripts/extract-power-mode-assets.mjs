#!/usr/bin/env node
// 一次性脚本:把 obsidian-power-mode 的 base64 GIF 解码成静态文件。
//
// 源项目把 13 个 GIF 以 base64 内嵌在 TS 源码里(约 233 KB)。note.md 这边要
// 的是文件:GIF 必须落在 dist/assets/power-mode/ 下,才能同时被主窗口
// (/assets/…) 和插件窗口的 Editor Kit (plugin://<id>/__host__/assets/…) 取到。
//
// 用法: node scripts/extract-power-mode-assets.mjs [源项目路径]
//   默认源路径 ~/git/obsidian-power-mode
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { homedir } from 'node:os'

const SRC = process.argv[2] ?? join(homedir(), 'git/obsidian-power-mode')
const OUT = resolve('public/assets/power-mode')
const PRESETS = { particle: 8, lightning: 3, coin: 1, confetti: 1 }

let total = 0
for (const [preset, expected] of Object.entries(PRESETS)) {
  const file = join(SRC, 'src/presets/explosion', `${preset}.ts`)
  const text = readFileSync(file, 'utf8')
  const matches = [...text.matchAll(/data:image\/gif;base64,([A-Za-z0-9+/=]+)/g)]
  if (matches.length !== expected) {
    throw new Error(`${preset}: expected ${expected} gifs, found ${matches.length}`)
  }
  const dir = join(OUT, preset)
  mkdirSync(dir, { recursive: true })
  matches.forEach((m, i) => {
    const buf = Buffer.from(m[1], 'base64')
    // GIF 魔数自检:解码错了就地失败,别产出一堆坏文件。
    const magic = buf.subarray(0, 6).toString('latin1')
    if (magic !== 'GIF87a' && magic !== 'GIF89a') {
      throw new Error(`${preset}/${i + 1}: not a GIF (magic=${magic})`)
    }
    writeFileSync(join(dir, `${i + 1}.gif`), buf)
    total++
  })
  console.log(`${preset}: ${matches.length} gifs`)
}
console.log(`✓ ${total} gifs → ${OUT}`)
