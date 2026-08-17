import { describe, it, expect } from 'vitest'
import { errorKey } from './errors'

// Exact wording the Rust backend emits (plugins-src/deepseek-agent/backend/src/).
// Pinning these here means a backend wording change that breaks the mapping
// fails this test instead of silently falling back to raw English.
describe('errorKey', () => {
  it('maps "no vault configured"', () => {
    expect(errorKey('no vault configured')).toBe('err.noVault')
  })

  it('maps a missing ACP server, whichever line of the hint matched', () => {
    // discover::NOT_FOUND, verbatim.
    const notFound =
      'DeepSeek Harness 的 ACP 服务端没找到。\n' +
      '装一个:`npm i -g @deepseek-ai/dsh-acp-demo`(开发者预览版),\n' +
      '或在插件设置里填 `dsh_acp_bin`(可执行路径)/ `dsh_repo`(monorepo checkout 路径),\n' +
      '也可以设环境变量 NOTEMD_DSH_ACP_BIN / DSH_REPO。'
    expect(errorKey(notFound)).toBe('err.harnessNotFound')
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
