import { describe, it, expect } from 'vitest'
import { errorKey } from './errors'

// Exact wording the Rust backend emits (plugins-src/codex-agent/backend/src/).
// Pinning these here means a backend wording change that breaks the mapping
// fails this test instead of silently falling back to raw English.
describe('errorKey', () => {
  it('maps "no vault configured"', () => {
    expect(errorKey('no vault configured')).toBe('err.noVault')
  })

  it('maps a missing Codex CLI, including the multiline installation hint', () => {
    const notFound =
      'Codex CLI 没找到。\n' +
      '请安装 Codex CLI (`npm i -g @openai/codex`),\n' +
      '或用环境变量 NOTEMD_CODEX_BIN 指定可执行文件。'
    expect(errorKey(notFound)).toBe('err.harnessNotFound')
    expect(errorKey('codex executable not found — install Codex CLI')).toBe(
      'err.harnessNotFound',
    )
  })

  it('maps a task whose policy will not parse', () => {
    expect(
      errorKey('/v/.notemd/agent-tasks/t/policy.json is not a valid policy: expected value at line 1'),
    ).toBe('err.badPolicy')
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
