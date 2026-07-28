import { describe, it, expect, beforeEach } from 'vitest'
import { answersStore, setAnswersFromText, clearAnswers, answeredMap } from './answers-store.svelte'

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

describe('answers store', () => {
  beforeEach(() => clearAnswers())

  it('starts empty', () => {
    expect(answersStore.entries).toHaveLength(0)
    expect(answeredMap().size).toBe(0)
  })

  it('parses note text into entries and bumps version', () => {
    const before = answersStore.version
    setAnswersFromText('/v/x.note.md', note)
    expect(answersStore.notePath).toBe('/v/x.note.md')
    expect(answersStore.entries).toHaveLength(1)
    expect(answeredMap().get('为什么?')?.body).toBe('因为如此。')
    expect(answersStore.version).toBeGreaterThan(before)
  })

  it('clearAnswers resets path and entries', () => {
    setAnswersFromText('/v/x.note.md', note)
    clearAnswers()
    expect(answersStore.notePath).toBeNull()
    expect(answersStore.entries).toHaveLength(0)
  })
})
