import { describe, expect, it } from 'vitest'
import { DEFAULT_WAITING_WARNING, DEFAULT_WIP_LIMIT, IDEA_STATES, ITEM_KINDS, NEXT_ACTIONS } from './model'

describe('Next v1 model contract', () => {
  it('keeps the persisted action vocabulary deliberately small', () => {
    expect(NEXT_ACTIONS).toEqual(['commit', 'wait', 'park', 'settle', 'reopen', 'relink'])
  })

  it('uses three as a soft WIP default, not as a type-level prohibition', () => {
    expect(DEFAULT_WIP_LIMIT).toBe(3)
    expect(DEFAULT_WAITING_WARNING).toBe(5)
    expect(IDEA_STATES).toContain('unsupported')
  })

  it('supports ideas and tasks as lifecycle item kinds', () => {
    expect(ITEM_KINDS).toEqual(['idea', 'task'])
  })
})
