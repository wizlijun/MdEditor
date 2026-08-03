import type { InsightRow } from './dashboard.svelte'
import { sessionMode } from './model'
import type { AudienceSession } from './audience'
import { CONCEPT_TYPE, conceptFileText } from '../okf/concept'

const MODE_CN: Record<'read' | 'edit' | 'mixed', string> = { read: '读', edit: '编', mixed: '读+编' }

export function fmtDuration(ms: number): string {
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ${s % 60}s`
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

function pad2(n: number): string {
  return String(n).padStart(2, '0')
}

/**
 * Format one attention interval in the device's LOCAL time as
 * `MM-DD HH:mm → HH:mm` (the end date is added only when it differs from the
 * start's, e.g. a session crossing midnight). `start`/`end` are epoch ms.
 */
export function fmtInterval(start: number, end: number): string {
  const a = new Date(start)
  const b = new Date(end)
  const day = (d: Date) => `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`
  const clock = (d: Date) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  const endLabel = day(a) === day(b) ? clock(b) : `${day(b)} ${clock(b)}`
  return `${day(a)} ${clock(a)} → ${endLabel}`
}

export function reportFilename(fromDay: string, toDay: string): string {
  return fromDay === toDay
    ? `stat/${fromDay}-daily-stat.md`
    : `stat/${fromDay}_${toDay}-stat.md`
}

/**
 * Build the "## 时间段" section: per doc, your read/edit intervals plus any
 * audience reading intervals (passed in — fetched by the report generator).
 * Docs with no intervals are omitted; returns [] when nothing to show.
 */
function intervalsSection(
  rows: InsightRow[],
  audByDoc: Record<string, AudienceSession[]>,
): string[] {
  const blocks = rows.flatMap((r) => {
    const aud = audByDoc[r.docKey] ?? []
    if (r.owner_sessions.length === 0 && aud.length === 0) return []
    const lines = [`- 《${r.label}》`]
    for (const s of r.owner_sessions) {
      lines.push(`  - ${fmtInterval(s.start, s.end)} · ${MODE_CN[sessionMode(s)]} · ${fmtDuration(s.read_ms + s.edit_ms)}`)
    }
    for (const s of aud) {
      lines.push(`  - 受众 ${fmtInterval(s.start, s.end)} · ${fmtDuration(s.ms)}`)
    }
    return lines
  })
  return blocks.length === 0 ? [] : ['## 时间段', '', ...blocks, '']
}

/** Render a deterministic Chinese reading digest for the range. `audSessionsByDoc`
 *  maps a row's docKey to its audience reading intervals (empty when unfetched). */
export function renderDailyReport(
  rows: InsightRow[],
  fromDay: string,
  toDay: string,
  audSessionsByDoc: Record<string, AudienceSession[]> = {},
): { filename: string; markdown: string } {
  const filename = reportFilename(fromDay, toDay)
  const rangeLabel = fromDay === toDay ? fromDay : `${fromDay} → ${toDay}`

  const head = (body: string): string =>
    conceptFileText({ type: CONCEPT_TYPE.readingReport, title: `阅读数据 · ${rangeLabel}` }, body)

  if (rows.length === 0) {
    return { filename, markdown: head(`# 阅读数据 · ${rangeLabel}\n\n此区间没有阅读或编辑记录。\n`) }
  }

  const totalEngage = rows.reduce((n, r) => n + r.read_ms + r.edit_ms, 0)
  const top = [...rows].sort((a, b) => (b.read_ms + b.edit_ms) - (a.read_ms + a.edit_ms))[0]
  const totalReaders = rows.reduce((n, r) => n + r.unique_readers, 0)
  const sharedRead = rows.reduce((n, r) => n + r.aud_read_ms, 0)
  const sharedCount = rows.filter((r) => r.shared).length

  let summary = `本区间你在 ${rows.length} 篇文档上共停留 ${fmtDuration(totalEngage)}，投入最多的是《${top.label}》。`
  if (totalReaders > 0) {
    summary += ` 其中 ${sharedCount} 篇分享文档共被 ${totalReaders} 位读者阅读 ${fmtDuration(sharedRead)}。`
  }

  const header = '| 文档 | 阅读 | 编辑 | 编辑段 | 标注 | 受众时长 | 读者 | 价值 |'
  const divider = '|---|---|---|---|---|---|---|---|'
  const body = rows.map((r) =>
    `| ${r.label}${r.shared ? ' 🔗' : ''} | ${fmtDuration(r.read_ms)} | ${fmtDuration(r.edit_ms)} | ${r.edit_sessions} | ${r.mark_ops} | ${fmtDuration(r.aud_read_ms)} | ${r.unique_readers} | ${r.value.toFixed(1)} |`,
  )
  const totals = `| **合计** | ${fmtDuration(rows.reduce((n, r) => n + r.read_ms, 0))} | ${fmtDuration(rows.reduce((n, r) => n + r.edit_ms, 0))} | ${rows.reduce((n, r) => n + r.edit_sessions, 0)} | ${rows.reduce((n, r) => n + r.mark_ops, 0)} | ${fmtDuration(sharedRead)} | ${totalReaders} | |`

  // Share URLs are attached below the table, anchored to their md file — the md
  // is the primary output, each URL a subordinate line (a doc may have several).
  const linkRows = rows.filter((r) => r.urls.length > 0)
  const links = linkRows.length === 0 ? [] : [
    '## 链接', '',
    ...linkRows.flatMap((r) => [`- 《${r.label}》`, ...r.urls.map((u) => `  - ${u}`)]),
    '',
  ]

  const markdown = head([
    `# 阅读数据 · ${rangeLabel}`, '', summary, '',
    header, divider, ...body, totals, '',
    ...intervalsSection(rows, audSessionsByDoc),
    ...links,
    '<sub>由 note.md Reading Insights 生成</sub>', '',
  ].join('\n'))

  return { filename, markdown }
}
