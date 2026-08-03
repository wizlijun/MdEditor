import { describe, it, expect } from 'vitest'
import { mergeFiles, aggregate, renderOwnerDigest, resolvePreset, collectSessions, fmtInterval } from './insights-report-core.mjs'
// @ts-expect-error - plain-JS lint core
import { lintText } from './okf-lint-core.mjs'

const files = [
  { name: '2026-07-08.DEV1.json', json: { deviceId: 'DEV1', deviceName: 'Mac', day: '2026-07-08', docs: { 'rel:a.md': { read_ms: 120000, edit_ms: 60000, edit_sessions: 2, mark_ops: 3, net_chars: 40, open_count: 1, first_seen_at: 0, last_active_at: 0 } } } },
  { name: '2026-07-08.DEV2.json', json: { deviceId: 'DEV2', deviceName: 'iPhone', day: '2026-07-08', docs: { 'rel:a.md': { read_ms: 30000, edit_ms: 0, edit_sessions: 0, mark_ops: 0, net_chars: 0, open_count: 1, first_seen_at: 0, last_active_at: 0 } } } },
  { name: '2026-07-07.DEV1.json', json: { deviceId: 'DEV1', deviceName: 'Mac', day: '2026-07-07', docs: { 'rel:a.md': { read_ms: 999, edit_ms: 0, edit_sessions: 0, mark_ops: 0, net_chars: 0, open_count: 1, first_seen_at: 0, last_active_at: 0 } } } },
]

describe('mergeFiles + aggregate', () => {
  it('sums a doc across devices for the range only', () => {
    const agg = aggregate(mergeFiles(files), '2026-07-08', '2026-07-08')
    expect(agg['rel:a.md'].read_ms).toBe(150000)
  })
})

describe('renderOwnerDigest', () => {
  it('produces a heading, the doc, and a total', () => {
    const agg = aggregate(mergeFiles(files), '2026-07-08', '2026-07-08')
    const md = renderOwnerDigest(agg, '2026-07-08', '2026-07-08')
    expect(md).toContain('# 阅读数据')
    expect(md).toContain('a.md')
    expect(md).toContain('合计')
    expect(md).toContain('2m 30s')
  })
})

describe('collectSessions + renderOwnerDigest intervals', () => {
  const sfiles = [
    { name: '2026-07-08.DEV1.json', json: { sessions: { 'rel:a.md': [
      { start: Date.UTC(2026, 6, 8, 1, 0), end: Date.UTC(2026, 6, 8, 1, 2), read_ms: 120000, edit_ms: 0 },
      { start: Date.UTC(2026, 6, 8, 6, 0), end: Date.UTC(2026, 6, 8, 6, 3), read_ms: 60000, edit_ms: 120000 },
    ] } } },
    { name: '2026-07-01.DEV1.json', json: { sessions: { 'rel:a.md': [
      { start: Date.UTC(2026, 6, 1, 1, 0), end: Date.UTC(2026, 6, 1, 1, 1), read_ms: 60000, edit_ms: 0 },
    ] } } },
  ]

  it('collects in-range intervals per doc, sorted, dropping out-of-range days', () => {
    const s = collectSessions(sfiles, '2026-07-08', '2026-07-08')
    expect(s['rel:a.md']).toHaveLength(2)
    expect(s['rel:a.md'][0].start).toBeLessThan(s['rel:a.md'][1].start)
  })

  it('renders a 时间段 section with mode-tagged intervals', () => {
    const md = renderOwnerDigest(
      aggregate(mergeFiles(files), '2026-07-08', '2026-07-08'),
      '2026-07-08', '2026-07-08',
      collectSessions(sfiles, '2026-07-08', '2026-07-08'),
    )
    expect(md).toContain('## 时间段')
    expect(md).toContain('读+编')
    expect(md).toMatch(/→/)
  })

  it('omits the 时间段 section when there are no intervals', () => {
    const md = renderOwnerDigest(aggregate(mergeFiles(files), '2026-07-08', '2026-07-08'), '2026-07-08', '2026-07-08')
    expect(md).not.toContain('## 时间段')
  })
})

describe('fmtInterval', () => {
  it('formats a same-day interval', () => {
    const start = new Date(2026, 6, 8, 9, 5).getTime()
    expect(fmtInterval(start, start + 25 * 60000)).toMatch(/^\d\d-\d\d \d\d:\d\d → \d\d:\d\d$/)
  })
})

describe('resolvePreset', () => {
  it('yesterday resolves to the prior day', () => {
    const now = Date.UTC(2026, 6, 8, 7, 0)
    expect(resolvePreset('yesterday', now, 480)).toEqual({ from: '2026-07-07', to: '2026-07-07' })
  })
})

describe('renderOwnerDigest — OKF frontmatter', () => {
  it('stamps a Reading Report concept head that passes the OKF hard constraints', () => {
    const agg = aggregate(mergeFiles(files), '2026-07-08', '2026-07-08')
    const md = renderOwnerDigest(agg, '2026-07-08', '2026-07-08')
    expect(md.startsWith('---\ntype: Reading Report\ntitle: 阅读数据 · 2026-07-08\n---\n')).toBe(true)
    expect(lintText('2026-07-08-daily-stat.md', md)).toEqual([])
  })
  it('stamps the empty-range digest too', () => {
    expect(lintText('empty.md', renderOwnerDigest({}, '2026-07-08', '2026-07-08'))).toEqual([])
  })
})
