import { describe, it, expect } from 'vitest'
import { markAdoptedInText } from './adopt-answer'
import { parseOutline } from '../outline/markdown'
import { parse as parseYaml } from 'yaml'

const note = [
  '- 原文',
  '  type:: annotation',
  '  - 为什么?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为如此。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

describe('markAdoptedInText', () => {
  it('flips the question status to adopted', () => {
    const out = markAdoptedInText(note, '为什么?')!
    expect(out).toContain('status:: adopted')
    expect(out).not.toContain('status:: answered')
  })

  it('keeps the answer node intact', () => {
    const out = markAdoptedInText(note, '为什么?')!
    const t = parseOutline(out)
    expect([...t.nodes.values()].some(n => n.source === 'answer')).toBe(true)
  })

  it('returns null when the question is not found (no write)', () => {
    expect(markAdoptedInText(note, '不存在的问题?')).toBeNull()
  })
})

describe('markAdoptedInText — 人工确认落成 OKF verified(§5.2)', () => {
  const AT = '2026-08-04T09:00:00.000Z'
  const withFm = `---\ntype: Outline Note\ntitle: t\n---\n${note}`

  it('records who adopted and when, in the document front-matter', () => {
    const out = markAdoptedInText(withFm, '为什么?', { by: 'human:bruce', at: AT })!
    const fm = parseYaml(out.slice(4, out.indexOf('\n---\n', 3)))
    expect(fm.verified).toEqual([{ by: 'human:bruce', at: AT }])
    expect(fm.type).toBe('Outline Note')
  })

  it('creates the front-matter block when the note has none', () => {
    const out = markAdoptedInText(note, '为什么?', { by: 'human:bruce', at: AT })!
    expect(out.startsWith(`---\nverified:\n  - by: human:bruce\n    at: ${AT}\n---\n`)).toBe(true)
    expect(parseOutline(out).nodes.size).toBe(parseOutline(note).nodes.size)
  })

  it('leaves the front-matter alone when no verifier is passed', () => {
    expect(markAdoptedInText(withFm, '为什么?')!).toContain('---\ntype: Outline Note\ntitle: t\n---\n')
  })

  it('appends a second adoption instead of replacing the first', () => {
    const once = markAdoptedInText(withFm, '为什么?', { by: 'human:bruce', at: AT })!
    const twice = markAdoptedInText(once, '为什么?', { by: 'human:bruce', at: '2026-08-05T09:00:00.000Z' })!
    const fm = parseYaml(twice.slice(4, twice.indexOf('\n---\n', 3)))
    expect(fm.verified).toHaveLength(2)
  })
})
