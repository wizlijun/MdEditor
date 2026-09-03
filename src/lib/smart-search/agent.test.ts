import { describe, expect, it, vi } from 'vitest'
import {
  AgentTaskError,
  normalizeAgentHarness,
  pollAgentTask,
  pollSearchAgentTask,
  startAgentTask,
  startSearchAgentTask,
} from './agent'

describe('smart-search agent transport', () => {
  it('drops malformed nested capabilities instead of trusting third-party JSON', () => {
    expect(normalizeAgentHarness({ harness: 'Third party', ok: true, capabilities: {
      tasks: ['search-plan'], model_routing: {},
    }})).toMatchObject({ harness: 'Third party', ok: true, capabilities: undefined })
    expect(normalizeAgentHarness({ ok: true })).toBeNull()
  })
  it('starts the shared search task', async () => {
    const transport = vi.fn(async () => ({ run_id: 'run-1' }))
    await expect(startSearchAgentTask('provider', 'prompt', 'result', transport)).resolves.toBe('run-1')
    expect(transport).toHaveBeenCalledWith('provider', 'run-task', {
      task: 'search-answer',
      prompt: 'prompt',
      usage_display: 'result',
    })
  })

  it('starts a planner with a per-invocation model profile and returns the real model', async () => {
    const transport = vi.fn(async () => ({ run_id: 'plan-1', resolved_model: 'fast-model' }))
    await expect(startAgentTask(
      'provider',
      'search-plan',
      'plan prompt',
      { model_profile: 'fast' },
      'result',
      transport,
    )).resolves.toEqual({ runId: 'plan-1', resolvedModel: 'fast-model' })
    expect(transport).toHaveBeenCalledWith('provider', 'run-task', {
      task: 'search-plan',
      prompt: 'plan prompt',
      usage_display: 'result',
      model_profile: 'fast',
    })
  })

  it('rejects contradictory model selectors before calling a provider', async () => {
    const transport = vi.fn()
    await expect(startAgentTask(
      'provider', 'search-plan', 'prompt',
      { model_profile: 'fast', model: 'other' } as never,
      'result', transport,
    )).rejects.toThrow('mutually exclusive')
    expect(transport).not.toHaveBeenCalled()
  })

  it('prefers the complete terminal result over the truncated record summary', async () => {
    const transport = vi.fn()
      .mockResolvedValueOnce({ state: 'running', steps: 2, last: 'Read a.md' })
      .mockResolvedValueOnce({
        state: 'done',
        record: { status: 'success', result: 'short' },
        terminal_result: { complete: true, content: 'complete answer' },
      })
    const progress = vi.fn()
    const result = await pollSearchAgentTask('provider', 'run-1', progress, { transport, intervalMs: 0 })
    expect(progress).toHaveBeenCalledWith({ steps: 2, last: 'Read a.md' })
    expect(result.content).toBe('complete answer')
  })

  it('requires a successful complete terminal result from the planner', async () => {
    const skipped = vi.fn().mockResolvedValue({
      state: 'done', record: { status: 'skipped', result: '{"schemaVersion":1}' },
    })
    await expect(pollAgentTask('provider', 'search-plan', 'p1', vi.fn(), {
      transport: skipped, intervalMs: 0,
    })).rejects.toThrow('planner run failed')

    const incomplete = vi.fn().mockResolvedValue({
      state: 'done', record: { status: 'success', result: '{"schemaVersion":1}' },
    })
    await expect(pollAgentTask('provider', 'search-plan', 'p2', vi.fn(), {
      transport: incomplete, intervalMs: 0,
    })).rejects.toThrow('no complete terminal result')
  })

  it('retries transient status reads for the same run without starting another task', async () => {
    const transport = vi.fn()
      .mockRejectedValueOnce(new Error('temporary IPC failure'))
      .mockResolvedValueOnce({ state: 'running', steps: 1, last: 'Still answering' })
      .mockResolvedValueOnce({
        state: 'done', record: { status: 'success' },
        terminal_result: { complete: true, content: 'answer' },
      })
    const retry = vi.fn()
    const progress = vi.fn()

    await expect(pollSearchAgentTask('provider', 'run-1', progress, {
      transport, intervalMs: 0, retryDelayMs: 0, onRetry: retry,
    })).resolves.toMatchObject({ runId: 'run-1', content: 'answer' })
    expect(retry).toHaveBeenCalledWith({
      attempt: 1, maxAttempts: 2, error: 'temporary IPC failure',
    })
    expect(progress).toHaveBeenCalledWith({ steps: 1, last: 'Still answering' })
    expect(transport).toHaveBeenCalledTimes(3)
    expect(transport.mock.calls.every((call) => call[1] === 'run-status')).toBe(true)
    expect(new Set(transport.mock.calls.map((call) => call[2].run_id))).toEqual(new Set(['run-1']))
  })

  it('preserves terminal timeout details as a typed task error and never reruns it', async () => {
    const transport = vi.fn().mockResolvedValue({
      state: 'done', record: { status: 'timeout', stderr_tail: 'quiet timeout after 180s' },
    })

    const failure = await pollSearchAgentTask('provider', 'answer-timeout', vi.fn(), {
      transport, intervalMs: 0,
    }).catch((error) => error)
    expect(failure).toBeInstanceOf(AgentTaskError)
    expect(failure).toMatchObject({
      task: 'search-answer', runId: 'answer-timeout', status: 'timeout',
      message: 'quiet timeout after 180s',
    })
    expect(transport).toHaveBeenCalledTimes(1)
  })

  it('stops after the bounded transient status retry budget', async () => {
    const transport = vi.fn().mockRejectedValue(new Error('bridge unavailable'))
    const failure = await pollAgentTask('provider', 'search-plan', 'p3', vi.fn(), {
      transport, intervalMs: 0, retryDelayMs: 0, maxTransientErrors: 2,
    }).catch((error) => error)

    expect(failure).toBeInstanceOf(AgentTaskError)
    expect(failure).toMatchObject({
      task: 'search-plan', runId: 'p3', status: 'transport_error',
    })
    expect(transport).toHaveBeenCalledTimes(3)
  })
})
