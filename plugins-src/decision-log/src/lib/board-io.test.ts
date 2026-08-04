import { describe, it, expect } from 'vitest'
import { serializeBoard, parseBoard, serializeArchive, parseArchive } from './board-io'
import type { OpenDecision, ArchivedDecision } from './model'

const dec: OpenDecision = {
  id: '2026-07-21-01', title: '先做 MVP', prediction: '两周内发出 MVP',
  confidence: 0.75, 'check-date': '2026-08-04', created: '2026-07-21',
  origin: 'manual', strikes: 0,
}

describe('board-io', () => {
  it('board round-trips through .note.md', () => {
    const md = serializeBoard([dec])
    expect(md).toMatch(/^---\n/)                 // front-matter first
    expect(md).toContain('type: Decision Board')
    expect(md).toContain('# 未决决策')            // human-readable mirror body
    expect(md).toContain('先做 MVP')
    expect(md).toContain('★★★ ≈75%')             // body shows stars + anchor, not raw float
    const back = parseBoard(md)
    expect(back).toHaveLength(1)
    expect(back[0]).toMatchObject({ id: '2026-07-21-01', prediction: '两周内发出 MVP', confidence: 0.75, strikes: 0 })
  })
  it('board round-trips optional progress[] notes', () => {
    const withProgress: OpenDecision = { ...dec, progress: [{ date: '2026-07-25', text: '进度过半' }] }
    const back = parseBoard(serializeBoard([withProgress]))
    expect(back[0].progress).toEqual([{ date: '2026-07-25', text: '进度过半' }])
  })
  it('board round-trips premortem + alternatives', () => {
    const full: OpenDecision = { ...dec, premortem: '范围太大', alternatives: ['先做插件'] }
    const back = parseBoard(serializeBoard([full]))
    expect(back[0].premortem).toBe('范围太大')
    expect(back[0].alternatives).toEqual(['先做插件'])
  })
  it('parseBoard normalizes legacy enum confidence to numeric', () => {
    const legacy = serializeBoard([dec]).replace('confidence: 0.75', 'confidence: medium')
    expect(parseBoard(legacy)[0].confidence).toBe(0.75)
  })
  it('parseBoard on empty/missing returns []', () => {
    expect(parseBoard('')).toEqual([])
    expect(parseBoard('# no frontmatter')).toEqual([])
  })
  it('archive round-trips (incl. weakest-element)', () => {
    const a: ArchivedDecision = { ...dec, status: 'closed', outcome: 'hit', 'still-endorse': false, 'weakest-element': 'alternatives' } as any
    const md = serializeArchive('2026-08-04', [a])
    expect(md).toContain('type: Decision Archive')
    expect(md).toContain('resolved: 2026-08-04')
    const back = parseArchive(md)
    expect(back[0]).toMatchObject({ id: '2026-07-21-01', outcome: 'hit', 'still-endorse': false, 'weakest-element': 'alternatives' })
  })
})
