#!/usr/bin/env node
import { readdir, readFile, mkdir, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { mergeFiles, aggregate, renderOwnerDigest, resolvePreset, collectSessions } from './insights-report-core.mjs'

function arg(name, def) {
  const i = process.argv.indexOf(name)
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : def
}
const has = (name) => process.argv.includes(name)

const vault = arg('--vault', process.env.MDEDITOR_VAULT)
if (!vault) { console.error('usage: insights-report.mjs --vault <path> [--date yesterday|today|7d|30d|month] [--from YYYY-MM-DD --to YYYY-MM-DD] [--stdout]'); process.exit(2) }

const tz = -new Date().getTimezoneOffset()
let from = arg('--from'), to = arg('--to')
if (!from || !to) { const r = resolvePreset(arg('--date', 'yesterday'), Date.now(), tz); from = r.from; to = r.to }

const dir = join(vault, '.notemd', 'analytics')
const files = []
if (existsSync(dir)) {
  for (const name of await readdir(dir)) {
    if (!name.endsWith('.json')) continue
    try { files.push({ name, json: JSON.parse(await readFile(join(dir, name), 'utf8')) }) } catch {}
  }
}
const md = renderOwnerDigest(aggregate(mergeFiles(files), from, to), from, to, collectSessions(files, from, to))

if (has('--stdout')) { process.stdout.write(md) }
else {
  const statDir = join(vault, 'stat')
  await mkdir(statDir, { recursive: true })
  const fname = from === to ? `${from}-daily-stat.md` : `${from}_${to}-stat.md`
  const out = join(statDir, fname)
  await writeFile(out, md)
  console.log(`wrote ${out}`)
}
