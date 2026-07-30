import { describe, it, expect } from 'vitest'
import { emptyView, reduce, type RunView } from './events'

const running = (runId = 'r1'): RunView => ({ ...emptyView(), runId, status: 'running' })

describe('run view reducer', () => {
  it('starts idle and empty', () => {
    expect(emptyView().status).toBe('idle')
    expect(emptyView().items).toEqual([])
  })

  it('appends a tool call as its own row', () => {
    const v = reduce(running(), {
      kind: 'event',
      run_id: 'r1',
      event: { kind: 'tool_use', name: 'Read', brief: 'a.md' },
    })
    expect(v.items).toEqual([{ type: 'tool', name: 'Read', brief: 'a.md' }])
  })

  it('merges consecutive text events into one row', () => {
    let v = reduce(running(), { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'he' } })
    v = reduce(v, { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'llo' } })
    expect(v.items).toEqual([{ type: 'text', text: 'hello' }])
  })

  it('starts a new text row after a tool call', () => {
    let v = reduce(running(), { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'a' } })
    v = reduce(v, {
      kind: 'event',
      run_id: 'r1',
      event: { kind: 'tool_use', name: 'Read', brief: '' },
    })
    v = reduce(v, { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'b' } })
    expect(v.items.map((i) => i.type)).toEqual(['text', 'tool', 'text'])
  })

  it('goes terminal on done and records the turn count', () => {
    const v = reduce(running(), {
      kind: 'done',
      run_id: 'r1',
      record: { status: 'success', num_turns: 7, result: 'done' },
    })
    expect(v.status).toBe('success')
    expect(v.turns).toBe(7)
    expect(v.result).toBe('done')
  })

  it('carries a failure through to the view', () => {
    const v = reduce(running(), {
      kind: 'done',
      run_id: 'r1',
      record: { status: 'timeout', result: 'took too long' },
    })
    expect(v.status).toBe('timeout')
    expect(v.result).toBe('took too long')
  })

  it('ignores messages from a different run', () => {
    const before = running()
    const after = reduce(before, {
      kind: 'event',
      run_id: 'OTHER',
      event: { kind: 'text', text: 'stray' },
    })
    expect(after).toBe(before)
  })

  it('accepts messages before the run id is known', () => {
    const v = reduce(emptyView(), {
      kind: 'event',
      run_id: 'r1',
      event: { kind: 'text', text: 'early' },
    })
    expect(v.items).toEqual([{ type: 'text', text: 'early' }])
  })

  it('surfaces a busy rejection', () => {
    const v = reduce(running(), {
      kind: 'busy',
      run_id: 'r1',
      holder: { run_id: 'r0', pid: 1, started_at: 'x' },
    })
    expect(v.status).toBe('busy')
  })

  it('drops system events rather than showing them as rows', () => {
    const v = reduce(running(), {
      kind: 'event',
      run_id: 'r1',
      event: { kind: 'system', subtype: 'init' },
    })
    expect(v.items).toEqual([])
  })
})
