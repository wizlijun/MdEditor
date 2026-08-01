import { describe, it, expect } from 'vitest'
import { errorKey } from './errors'

// Exact wording the Rust backend emits (plugins-src/claude-agent/backend/src/plugin.rs).
// Pinning these here means a backend wording change that breaks the mapping
// fails this test instead of silently falling back to raw English.
describe('errorKey', () => {
  it('maps "no vault configured"', () => {
    expect(errorKey('no vault configured')).toBe('err.noVault')
  })

  it('maps a missing claude executable', () => {
    expect(
      errorKey('claude executable not found — install Claude Code, or point NOTEMD_CLAUDE_BIN at it'),
    ).toBe('err.claudeNotFound')
  })

  it('maps an unknown task id', () => {
    expect(errorKey("unknown task 'gone-task'")).toBe('err.unknownTask')
  })

  it('maps cancelling a run that already finished', () => {
    expect(errorKey("run 'R1' is not running")).toBe('err.notRunning')
  })

  it('leaves an unrecognized message unmatched', () => {
    expect(errorKey('missing \'task\'')).toBe(null)
    expect(errorKey('some future backend string')).toBe(null)
  })
})
