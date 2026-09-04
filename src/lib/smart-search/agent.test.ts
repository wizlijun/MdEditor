import { describe, expect, it, vi } from 'vitest'
import {
  AgentTaskError,
  cancelAgentTask,
  normalizeAgentHarness,
  pollAgentTask,
  startAgentTask,
  supportsSearchPlanner,
} from './agent'

describe('smart-search agent transport', () => {
  it('drops malformed nested capabilities instead of trusting third-party JSON', () => {
    expect(normalizeAgentHarness({ harness: 'Third party', ok: true, capabilities: {
      tasks: ['search-plan'], model_routing: {},
    }})).toMatchObject({ harness: 'Third party', ok: true, capabilities: undefined })
    expect(normalizeAgentHarness({ ok: true })).toBeNull()
  })

  it('keeps a Planner available when its explicit fast profile works without a catalog default', () => {
    const harness = normalizeAgentHarness({
      harness: 'Codex CLI',
      ok: true,
      default_model: null,
      capabilities: {
        tasks: ['search-plan'],
        search_plan_schemas: [1],
        terminal_result: true,
        input_only_isolation: true,
        model_routing: {
          invocation_override: true,
          profiles: {
            fast: { model: 'gpt-fast', available: true },
            default: { model: null, available: false },
          },
          selectable_models: [],
        },
      },
    })
    expect(supportsSearchPlanner({ id: 'codex', name: 'Codex', harness })).toBe(true)
  })

  it('starts each task with an invocation id and an input hash', async () => {
    const transport = vi.fn(async () => ({ run_id: 'plan-1', resolved_model: 'fast-model' }))
    await expect(startAgentTask(
      'provider', 'search-plan', 'plan prompt', { model_profile: 'fast' }, 'result', transport,
      { invocationId: 'lookup-1', inputHash: 'a'.repeat(64) },
    )).resolves.toEqual({ runId: 'plan-1', resolvedModel: 'fast-model' })
    expect(transport).toHaveBeenCalledWith('provider', 'run-task', {
      task: 'search-plan', prompt: 'plan prompt', usage_display: 'result',
      invocation_id: 'lookup-1', input_hash: 'a'.repeat(64), model_profile: 'fast',
    })
  })

  it('rejects contradictory model selectors before calling a provider', async () => {
    const transport = vi.fn()
    await expect(startAgentTask(
      'provider', 'search-plan', 'prompt',
      { model_profile: 'fast', model: 'other' } as never, 'result', transport,
    )).rejects.toThrow('mutually exclusive')
    expect(transport).not.toHaveBeenCalled()
  })

  it('uses the idempotent cancellation command', async () => {
    const transport = vi.fn(async () => ({ ok: true }))
    await cancelAgentTask('provider', 'search-plan', 'run-1', transport)
    expect(transport).toHaveBeenCalledWith('provider', 'run-cancel', {
      task: 'search-plan', run_id: 'run-1',
    })
  })

  it('requires complete terminal results for plan and summary', async () => {
    const incomplete = vi.fn().mockResolvedValue({
      state: 'done', record: { status: 'success', result: 'partial' },
    })
    await expect(pollAgentTask('provider', 'search-plan', 'p1', vi.fn(), {
      transport: incomplete, intervalMs: 0,
    })).rejects.toThrow('no complete terminal result')
    await expect(pollAgentTask('provider', 'search-summary', 's1', vi.fn(), {
      transport: incomplete, intervalMs: 0,
    })).rejects.toThrow('no complete terminal result')
  })

  it('retries transient status reads for the same run only', async () => {
    const transport = vi.fn()
      .mockRejectedValueOnce(new Error('temporary IPC failure'))
      .mockResolvedValueOnce({ state: 'running', steps: 1, last: 'private text' })
      .mockResolvedValueOnce({
        state: 'done', record: { status: 'success' },
        terminal_result: { complete: true, content: '{"schemaVersion":1}' },
      })
    const retry = vi.fn()
    const progress = vi.fn()
    await expect(pollAgentTask('provider', 'search-plan', 'run-1', progress, {
      transport, intervalMs: 0, retryDelayMs: 0, onRetry: retry,
    })).resolves.toMatchObject({ runId: 'run-1' })
    expect(retry).toHaveBeenCalledTimes(1)
    expect(progress).toHaveBeenCalledWith({ steps: 1, last: 'private text' })
    expect(new Set(transport.mock.calls.map((call) => call[2].run_id))).toEqual(new Set(['run-1']))
  })

  it('preserves a terminal timeout as a typed error', async () => {
    const transport = vi.fn().mockResolvedValue({
      state: 'done', record: { status: 'timeout', stderr_tail: 'quiet timeout' },
    })
    const failure = await pollAgentTask('provider', 'search-summary', 's1', vi.fn(), {
      transport, intervalMs: 0,
    }).catch((error) => error)
    expect(failure).toBeInstanceOf(AgentTaskError)
    expect(failure).toMatchObject({ task: 'search-summary', runId: 's1', status: 'timeout' })
    expect(failure.message).toBe('search-summary run timed out')
  })

  it('does not present non-fatal provider stderr as a cancelled Planner cause', async () => {
    const transport = vi.fn().mockResolvedValue({
      state: 'done',
      record: {
        status: 'cancelled',
        stderr_tail: 'ERROR codex_models_manager::manager: failed to refresh available models',
      },
    })
    const failure = await pollAgentTask('provider', 'search-plan', 'p1', vi.fn(), {
      transport, intervalMs: 0,
    }).catch((error) => error)
    expect(failure).toBeInstanceOf(AgentTaskError)
    expect(failure.message).toBe('search-plan run was cancelled')
    expect(failure.message).not.toContain('codex_models_manager')
  })
})
