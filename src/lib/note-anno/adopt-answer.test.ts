import { describe, it, expect } from 'vitest'
import { markAdoptedInText } from './adopt-answer'
import { parseOutline } from '../outline/markdown'

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
