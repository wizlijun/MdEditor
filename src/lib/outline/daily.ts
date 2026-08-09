// src/lib/outline/daily.ts
import { joinPath } from '../fs'

export type DateLinkKind = 'day' | 'month' | 'year'

const DAY_RE = /^(\d{4})-(\d{2})-(\d{2})$/
const MONTH_RE = /^(\d{4})-(\d{2})$/
const YEAR_RE = /^(\d{4})$/

/**
 * 日期链接规范形式(spec §6):[[yyyy-MM-dd]]/[[yyyy-MM]]/[[yyyy]] 三种,
 * 其余日期写法一律不解析。做月/日范围粗验(01-12 / 01-31)。
 */
export function parseDateLink(target: string): { kind: DateLinkKind; year: string } | null {
  let m = target.match(DAY_RE)
  if (m) {
    const mm = Number(m[2]), dd = Number(m[3])
    return mm >= 1 && mm <= 12 && dd >= 1 && dd <= 31 ? { kind: 'day', year: m[1] } : null
  }
  m = target.match(MONTH_RE)
  if (m) {
    const mm = Number(m[2])
    return mm >= 1 && mm <= 12 ? { kind: 'month', year: m[1] } : null
  }
  m = target.match(YEAR_RE)
  return m ? { kind: 'year', year: m[1] } : null
}

/** vault/{dailynoteDir}/{yyyy}/{target}.note.md;非日期返回 null */
export function dailyNotePath(vaultRoot: string, dailynoteDir: string, target: string): string | null {
  const d = parseDateLink(target)
  if (!d) return null
  return joinPath(joinPath(joinPath(vaultRoot, dailynoteDir), d.year), `${target}.note.md`)
}

/** 本地时区 yyyy-MM-dd(文件名字典序即时间序,spec §6) */
export function todayStr(d: Date = new Date()): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

/**
 * 确保日期笔记存在(按需建年目录;fm title = 日期字符串本身),返回路径。
 * vault 未配置返回 null。IO 薄层,vitest 不覆盖(仓库惯例)。
 */
export async function ensureDailyNote(target: string): Promise<string | null> {
  const { sotvaultStore } = await import('../sotvault.svelte')
  const vault = sotvaultStore.vaultRoot
  if (!vault) return null
  const { outlineDirs } = await import('./dirs.svelte')
  const path = dailyNotePath(vault, outlineDirs.dailynote, target)
  if (!path) return null
  const { mkdir } = await import('@tauri-apps/plugin-fs')
  const { dirname } = await import('../paths')
  await mkdir(dirname(path), { recursive: true }).catch(() => {})
  const { ensureOutlineFile } = await import('./create')
  const { CONCEPT_TYPE } = await import('../okf/concept')
  await ensureOutlineFile(path, target, CONCEPT_TYPE.dailyNote)
  return path
}
