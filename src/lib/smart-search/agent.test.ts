import { describe, expect, it, vi } from 'vitest'
import { pollSearchAgentTask, startSearchAgentTask } from './agent'

describe('smart-search agent transport', () => {
  it('starts the shared search task', async () => {
    const transport = vi.fn(async () => ({ run_id: 'run-1' }))
    await expect(startSearchAgentTask('provider', 'prompt', 'result', transport)).resolves.toBe('run-1')
    expect(transport).toHaveBeenCalledWith('provider', 'run-task', {
      task: 'search-answer',
      prompt: 'prompt',
      usage_display: 'result',
    })
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
})
