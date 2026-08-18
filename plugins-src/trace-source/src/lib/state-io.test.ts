import { describe, expect, it } from 'vitest'
import { DEFAULT_STATE, normalizeTraceDir, parseState, serializeState, STATE_PATH } from './state-io'

describe('parseState', () => {
  it('默认 traceDir=inbox/traces、inbox 关闭', () => {
    expect(DEFAULT_STATE.traceDir).toBe('inbox/traces')
    expect(parseState(null)).toEqual({ traceDir: 'inbox/traces', inboxOpen: false })
    expect(STATE_PATH).toBe('.notemd/trace-source.json')
  })

  it('坏 JSON/非对象/坏键逐项回退默认,绝不抛', () => {
    expect(parseState('')).toEqual(DEFAULT_STATE)
    expect(parseState('not json')).toEqual(DEFAULT_STATE)
    expect(parseState('[1]')).toEqual(DEFAULT_STATE)
    expect(parseState('{"traceDir": 3, "inboxOpen": "yes"}')).toEqual(DEFAULT_STATE)
    expect(parseState('{"traceDir": "my/dir", "inboxOpen": true}')).toEqual({
      traceDir: 'my/dir',
      inboxOpen: true,
    })
  })

  it('serialize→parse 往返一致', () => {
    const s = { traceDir: 'notes/traces', inboxOpen: true }
    expect(parseState(serializeState(s))).toEqual(s)
  })
})

describe('normalizeTraceDir', () => {
  it('拒绝绝对路径、.. 与空;去掉尾斜杠', () => {
    expect(normalizeTraceDir('/abs')).toBeNull()
    expect(normalizeTraceDir('a/../b')).toBeNull()
    expect(normalizeTraceDir('   ')).toBeNull()
    expect(normalizeTraceDir('inbox/traces/')).toBe('inbox/traces')
    expect(normalizeTraceDir('inbox/traces')).toBe('inbox/traces')
  })
})
