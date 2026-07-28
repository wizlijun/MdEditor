import { describe, it, expect } from 'vitest'
import { deriveAnswers, answeredByNoteText } from './answers'
import { parseOutline } from './markdown'

const note = [
  '- 原文一',
  '  type:: annotation',
  '  line:: 3',
  '  - 为什么能到 90%?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为前缀重复。',
  '      ```',
  '      type:: answer',
  '      by:: claude-code',
  '  - 还没答的问题?',
  '    type:: question',
  '    status:: open',
  '- 原文二',
  '  type:: annotation',
  '  line:: 9',
  '  - 已采纳的问题?',
  '    type:: question',
  '    status:: adopted',
  '    - ```markdown',
  '      旧答复。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

describe('deriveAnswers', () => {
  it('returns one entry per question that has an answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.map(r => r.noteText).sort())
      .toEqual(['为什么能到 90%?', '已采纳的问题?'].sort())
  })

  it('carries body (fence stripped), status and author', () => {
    const rows = deriveAnswers(parseOutline(note))
    const r = rows.find(x => x.noteText === '为什么能到 90%?')!
    expect(r.body).toBe('因为前缀重复。')
    expect(r.status).toBe('answered')
    expect(r.by).toBe('claude-code')
    expect(r.questionId).toBeTruthy()
  })

  it('skips questions with no answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.some(r => r.noteText === '还没答的问题?')).toBe(false)
  })

  it('answeredByNoteText only exposes status answered', () => {
    const map = answeredByNoteText(deriveAnswers(parseOutline(note)))
    expect(map.has('为什么能到 90%?')).toBe(true)
    expect(map.has('已采纳的问题?')).toBe(false)   // adopted 不再出卡片
  })
})
