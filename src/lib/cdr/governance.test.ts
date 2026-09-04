import { describe, expect, it } from 'vitest'
import { actorKey, intersectDecisions, parseGovernanceDecision } from './governance'

describe('CDR governance values', () => {
  it('takes the least privilege across authorization, profile and request mode', () => {
    expect(intersectDecisions('apply', 'apply', 'apply')).toBe('apply')
    expect(intersectDecisions('apply', 'apply', 'propose')).toBe('propose')
    expect(intersectDecisions('apply', 'propose', 'apply')).toBe('propose')
    expect(intersectDecisions('deny', 'apply', 'apply')).toBe('deny')
    expect(() => intersectDecisions('unexpected' as never, 'apply', 'apply'))
      .toThrow('CDR_GOVERNANCE_DECISION_INVALID')
    expect(() => parseGovernanceDecision(undefined)).toThrow('CDR_GOVERNANCE_DECISION_INVALID')
  })

  it('builds a stable actor key from a bound actor value', () => {
    expect(actorKey({ kind: 'agent', id: 'organizer/run-1' })).toBe('agent:organizer/run-1')
    expect(actorKey({ kind: 'human', id: '李雷' })).toBe('human:李雷')
    expect(actorKey({ kind: 'human', id: 'foo+bar' })).toBe('human:foo+bar')
    expect(() => actorKey({ kind: 'human', id: '' })).toThrow('CDR_ACTOR_INVALID')
    expect(() => actorKey({ kind: 'human', id: 'two words' })).toThrow('CDR_ACTOR_INVALID')
    expect(() => actorKey({ kind: 'remote' as never, id: 'someone' })).toThrow('CDR_ACTOR_INVALID')
  })
})
