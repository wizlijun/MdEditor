import { describe, it, expect } from 'vitest'
import { parseState, serializeState, DEFAULT_STATE, STATE_PATH, type SparkState } from './state-io'

describe('STATE_PATH', () => {
  it('is the vault-relative config path', () => {
    expect(STATE_PATH).toBe('.notemd/idea-spark.json')
  })
})

describe('parseState — happy path', () => {
  it('round-trips a well-formed state through serialize/parse', () => {
    const s: SparkState = {
      ideaDir: 'inbox/ideas',
      pendingRuns: { 'inbox/ideas/a.md': 'run-1' },
      inboxOpen: false,
      placeholderSeq: 0,
    }
    expect(parseState(serializeState(s))).toEqual(s)
  })
})

describe('parseState — falls back to defaults, never throws', () => {
  it('handles null (file did not exist yet)', () => {
    expect(parseState(null)).toEqual(DEFAULT_STATE)
  })

  it('handles an empty string', () => {
    expect(parseState('')).toEqual(DEFAULT_STATE)
  })

  it('handles unparseable JSON', () => {
    expect(() => parseState('{not json')).not.toThrow()
    expect(parseState('{not json')).toEqual(DEFAULT_STATE)
  })

  it('handles JSON that is not an object (array, string, number)', () => {
    expect(parseState('[1,2,3]')).toEqual(DEFAULT_STATE)
    expect(parseState('"hello"')).toEqual(DEFAULT_STATE)
    expect(parseState('42')).toEqual(DEFAULT_STATE)
    expect(parseState('null')).toEqual(DEFAULT_STATE)
  })

  it('merges a partial object — only ideaDir present', () => {
    expect(parseState(JSON.stringify({ ideaDir: 'other/dir' }))).toEqual({
      ideaDir: 'other/dir',
      pendingRuns: {},
      inboxOpen: false,
      placeholderSeq: 0,
    })
  })

  it('merges a partial object — only pendingRuns present', () => {
    expect(parseState(JSON.stringify({ pendingRuns: { 'a.md': 'run-9' } }))).toEqual({
      ideaDir: DEFAULT_STATE.ideaDir,
      pendingRuns: { 'a.md': 'run-9' },
      inboxOpen: false,
      placeholderSeq: 0,
    })
  })

  it('falls back ideaDir to default when it has the wrong type', () => {
    expect(parseState(JSON.stringify({ ideaDir: 42, pendingRuns: {} }))).toEqual(DEFAULT_STATE)
  })

  it('falls back pendingRuns to {} when it has the wrong type (array, string, or non-string values)', () => {
    expect(parseState(JSON.stringify({ ideaDir: 'd', pendingRuns: ['a', 'b'] }))).toEqual({
      ideaDir: 'd',
      pendingRuns: {},
      inboxOpen: false,
      placeholderSeq: 0,
    })
    expect(parseState(JSON.stringify({ ideaDir: 'd', pendingRuns: 'nope' }))).toEqual({
      ideaDir: 'd',
      pendingRuns: {},
      inboxOpen: false,
      placeholderSeq: 0,
    })
    expect(parseState(JSON.stringify({ ideaDir: 'd', pendingRuns: { 'a.md': 7 } }))).toEqual({
      ideaDir: 'd',
      pendingRuns: {},
      inboxOpen: false,
      placeholderSeq: 0,
    })
  })

  it('never mutates the shared DEFAULT_STATE constant across calls', () => {
    const a = parseState(null)
    a.pendingRuns['a.md'] = 'run-1'
    a.ideaDir = 'mutated'
    expect(parseState(null)).toEqual(DEFAULT_STATE)
    expect(DEFAULT_STATE).toEqual({
      ideaDir: 'inbox/ideas',
      pendingRuns: {},
      inboxOpen: false,
      placeholderSeq: 0,
    })
  })

  it('defaults the new fields and tolerates wrong types', () => {
    expect(parseState(null).inboxOpen).toBe(false)
    expect(parseState(null).placeholderSeq).toBe(0)
    expect(parseState('{"inboxOpen":"yes","placeholderSeq":"x"}').inboxOpen).toBe(false)
    expect(parseState('{"inboxOpen":"yes","placeholderSeq":"x"}').placeholderSeq).toBe(0)
    expect(parseState('{"inboxOpen":true,"placeholderSeq":7}').placeholderSeq).toBe(7)
  })

  it('round-trips the new fields', () => {
    const s = { ...DEFAULT_STATE, inboxOpen: true, placeholderSeq: 3 }
    expect(parseState(serializeState(s))).toEqual(s)
  })
})

describe('serializeState', () => {
  it('produces JSON parseable back to the same shape', () => {
    const s: SparkState = { ideaDir: 'inbox/ideas', pendingRuns: {}, inboxOpen: false, placeholderSeq: 0 }
    expect(JSON.parse(serializeState(s))).toEqual(s)
  })
})
