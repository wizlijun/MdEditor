import { describe, it, expect } from 'vitest'
import { reconcileGraphChoice } from './graph-choice'

describe('reconcileGraphChoice', () => {
  /** R5, the regression this function exists for. `probe` answers with an
   *  empty graph list whenever the Roam desktop app is not running (states
   *  `missing` and `not_connected`) — a success, not a failure, so the UI used
   *  to run its "reconcile" over it and wipe the pick. */
  it('leaves the pick alone when the probe reports no graphs at all', () => {
    expect(reconcileGraphChoice('work', [])).toBe('work')
    expect(reconcileGraphChoice('', [])).toBe('')
  })

  it('keeps a pick the probe confirms', () => {
    expect(reconcileGraphChoice('work', ['personal', 'work'])).toBe('work')
    expect(reconcileGraphChoice('work', ['work'])).toBe('work')
  })

  it('replaces a pick the probe contradicts', () => {
    // The graph is gone from a multi-graph machine: offer the first, which the
    // picker (shown whenever there is more than one) then displays.
    expect(reconcileGraphChoice('gone', ['personal', 'work'])).toBe('personal')
    // Back down to a single graph: no choice to make, so let the CLI
    // auto-select rather than sending a name that would fail every sync.
    expect(reconcileGraphChoice('gone', ['work'])).toBe('')
  })

  it('preselects one for a multi-graph machine that has never picked', () => {
    expect(reconcileGraphChoice('', ['personal', 'work'])).toBe('personal')
    expect(reconcileGraphChoice('', ['work'])).toBe('')
  })

  /** The sequence R5 describes, end to end: two graphs, the user picks the
   *  second, opens the window while Roam is closed, and syncs once it is up
   *  again. The day must come from the graph they chose. */
  it('survives a probe taken while the Roam app was closed', () => {
    let pick = 'work'
    pick = reconcileGraphChoice(pick, ['personal', 'work'])
    pick = reconcileGraphChoice(pick, []) // window opened, Roam not running
    pick = reconcileGraphChoice(pick, ['personal', 'work'])
    expect(pick).toBe('work')
  })
})
