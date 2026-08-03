export function fmtDuration(ms) {
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ${s % 60}s`
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

const FILE_RE = /^(\d{4}-\d{2}-\d{2})\.(.+)\.json$/

/** files: [{ name, json }] → docKey -> day -> summed counters (across devices). */
export function mergeFiles(files) {
  const out = {}
  for (const f of files) {
    const m = FILE_RE.exec(f.name)
    if (!m || !f.json || !f.json.docs) continue
    const day = m[1]
    for (const [docKey, c] of Object.entries(f.json.docs)) {
      const perDoc = (out[docKey] ??= {})
      const b = (perDoc[day] ??= { read_ms: 0, edit_ms: 0, edit_sessions: 0, mark_ops: 0, net_chars: 0, open_count: 0 })
      b.read_ms += c.read_ms || 0
      b.edit_ms += c.edit_ms || 0
      b.edit_sessions += c.edit_sessions || 0
      b.mark_ops += c.mark_ops || 0
      b.net_chars += c.net_chars || 0
      b.open_count += c.open_count || 0
    }
  }
  return out
}

/** Sum each doc over the inclusive [from,to] day range (lexicographic). */
export function aggregate(merged, fromDay, toDay) {
  const out = {}
  for (const [docKey, days] of Object.entries(merged)) {
    let acc = null
    for (const [day, c] of Object.entries(days)) {
      if (day < fromDay || day > toDay) continue
      if (!acc) acc = { read_ms: 0, edit_ms: 0, edit_sessions: 0, mark_ops: 0, net_chars: 0, open_count: 0 }
      for (const k of Object.keys(acc)) acc[k] += c[k] || 0
    }
    if (acc) out[docKey] = acc
  }
  return out
}

function label(docKey) {
  const p = docKey.replace(/^(rel:|abs:)/, '')
  const i = p.lastIndexOf('/')
  return i >= 0 ? p.slice(i + 1) : p
}

const pad2 = (n) => String(n).padStart(2, '0')

/** Format one interval in LOCAL time as `MM-DD HH:mm → HH:mm` (end date added
 *  when it lands on another day). start/end are epoch ms. */
export function fmtInterval(start, end) {
  const a = new Date(start), b = new Date(end)
  const day = (d) => `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`
  const clock = (d) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  const endLabel = day(a) === day(b) ? clock(b) : `${day(b)} ${clock(b)}`
  return `${day(a)} ${clock(a)} → ${endLabel}`
}

function modeCn(s) {
  if (s.read_ms > 0 && s.edit_ms > 0) return '读+编'
  return s.edit_ms > 0 ? '编' : '读'
}

/** files → docKey -> in-range attention intervals (merged across devices, sorted). */
export function collectSessions(files, fromDay, toDay) {
  const out = {}
  for (const f of files) {
    if (!f.json || !f.json.sessions) continue
    const m = FILE_RE.exec(f.name)
    if (!m) continue
    const day = m[1]
    if (day < fromDay || day > toDay) continue
    for (const [docKey, list] of Object.entries(f.json.sessions)) {
      ;(out[docKey] ??= []).push(...list)
    }
  }
  for (const list of Object.values(out)) list.sort((a, b) => a.start - b.start)
  return out
}

export function renderOwnerDigest(agg, fromDay, toDay, sessionsByDoc = {}) {
  const rangeLabel = fromDay === toDay ? fromDay : `${fromDay} → ${toDay}`
  // OKF 概念文档头(§4.1 必填 type);type 取值登记在 src/lib/okf/concept.ts
  const head = (body) => `---\ntype: Reading Report\ntitle: 阅读数据 · ${rangeLabel}\n---\n${body}`
  const rows = Object.entries(agg).map(([docKey, c]) => ({ docKey, label: label(docKey), ...c }))
    .sort((a, b) => (b.read_ms + b.edit_ms) - (a.read_ms + a.edit_ms))
  if (rows.length === 0) return head(`# 阅读数据 · ${rangeLabel}\n\n此区间没有阅读或编辑记录。\n`)
  const totalEngage = rows.reduce((n, r) => n + r.read_ms + r.edit_ms, 0)
  const summary = `本区间你在 ${rows.length} 篇文档上共停留 ${fmtDuration(totalEngage)}，投入最多的是《${rows[0].label}》。`
  const header = '| 文档 | 阅读 | 编辑 | 编辑段 | 标注 |'
  const divider = '|---|---|---|---|---|'
  const body = rows.map((r) => `| ${r.label} | ${fmtDuration(r.read_ms)} | ${fmtDuration(r.edit_ms)} | ${r.edit_sessions} | ${r.mark_ops} |`)
  const totals = `| **合计** | ${fmtDuration(rows.reduce((n, r) => n + r.read_ms, 0))} | ${fmtDuration(rows.reduce((n, r) => n + r.edit_ms, 0))} | ${rows.reduce((n, r) => n + r.edit_sessions, 0)} | ${rows.reduce((n, r) => n + r.mark_ops, 0)} |`

  // "## 时间段": per doc, your read/edit intervals (owner view — no audience here).
  const blocks = rows.flatMap((r) => {
    const list = sessionsByDoc[r.docKey] ?? []
    if (list.length === 0) return []
    return [`- 《${r.label}》`, ...list.map((s) => `  - ${fmtInterval(s.start, s.end)} · ${modeCn(s)} · ${fmtDuration(s.read_ms + s.edit_ms)}`)]
  })
  const intervals = blocks.length === 0 ? [] : ['## 时间段', '', ...blocks, '']

  return head([`# 阅读数据 · ${rangeLabel}`, '', summary, '', header, divider, ...body, totals, '', ...intervals, '<sub>由 note.md Reading Insights CLI 生成</sub>', ''].join('\n'))
}

function dayKey(ms, tz) {
  const d = new Date(ms + tz * 60000)
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, '0')}-${String(d.getUTCDate()).padStart(2, '0')}`
}
function addDays(day, delta) { return new Date(Date.parse(day + 'T00:00:00Z') + delta * 86400000).toISOString().slice(0, 10) }

/** preset: today|yesterday|7d|30d|month → { from, to } */
export function resolvePreset(preset, now, tz) {
  const today = dayKey(now, tz)
  switch (preset) {
    case 'today': return { from: today, to: today }
    case 'yesterday': { const y = addDays(today, -1); return { from: y, to: y } }
    case '7d': return { from: addDays(today, -6), to: today }
    case '30d': return { from: addDays(today, -29), to: today }
    case 'month': return { from: today.slice(0, 8) + '01', to: today }
    default: return { from: today, to: today }
  }
}
