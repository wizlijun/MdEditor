// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  completedInference,
  detectInferenceMode,
  interpretInferenceStatus,
  memoryInferenceStatus,
  startMemoryInference,
} from './inference'
import { MEMORY_INFERENCE_STATE } from './inference-task'

afterEach(() => { vi.restoreAllMocks() })

function bridge(handler: (method: string, params?: any) => Promise<any>) {
  const request = vi.fn(handler)
  window.notemd = { pluginId: 'notemd.memory', locale: 'zh', theme: 'system', request, onMessage: () => {} }
  return request
}

const state = (invocation_id = 'old') => JSON.stringify({
  schema: 'notemd.memory/inference-state/v2', invocation_id,
  last_successful_head: 'abc123', complete: true,
})

describe('Memory inference client', () => {
  it('uses only a valid successful scan state to choose incremental mode', async () => {
    let content = state()
    bridge(async (method) => method === 'host.vault.exists' ? { exists: true } : { content })
    expect(await detectInferenceMode()).toBe('incremental')
    content = '{broken'
    expect(await detectInferenceMode()).toBe('full')
  })

  it('seeds every task file before running and pins the selected harness', async () => {
    const request = bridge(async (method) => {
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') return { run_id: 'run-1' }
      return {}
    })
    const result = await startMemoryInference({ mode: 'full', harness: 'notemd.codex-agent' })
    expect(result.ok).toBe(true)
    const methods = request.mock.calls.map(([method]) => method)
    expect(methods.lastIndexOf('host.vault.write')).toBeLessThan(methods.indexOf('host.agent.run'))
    const call = request.mock.calls.find(([method]) => method === 'host.agent.run')!
    expect(call[1]).toMatchObject({ task: 'memory-inference', harness: 'notemd.codex-agent' })
    expect(call[1].prompt).toContain('Mode: full')
    expect(call[1].prompt).toContain('Only pending proposals are allowed')
    expect(call[1]).not.toHaveProperty('notify')
  })

  it('routes status to the same provider and validates this invocation checkpoint', async () => {
    const request = bridge(async (method, params) => {
      if (method === 'host.agent.status') return { state: 'running', steps: 2, last: 'scan' }
      if (method === 'host.vault.exists') return { exists: true }
      if (method === 'host.vault.read') return { content: state('invocation-1') }
      return {}
    })
    expect(interpretInferenceStatus(await memoryInferenceStatus('run-1', 'notemd.deepseek-agent'))).toEqual({ kind: 'running', steps: 2, last: 'scan' })
    expect(request).toHaveBeenCalledWith('host.agent.status', { task: 'memory-inference', run_id: 'run-1', harness: 'notemd.deepseek-agent' })
    expect(await completedInference('invocation-1')).toBe(true)
    expect(await completedInference('another')).toBe(false)
  })

  it('maps terminal, malformed and unavailable-agent outcomes without throwing', async () => {
    expect(interpretInferenceStatus({ state: 'done', record: { status: 'success', result: 'ok' } })).toEqual({ kind: 'done', success: true, message: 'ok' })
    expect(interpretInferenceStatus({ state: 'done', record: { status: 'error', stderr_tail: 'bad' } })).toEqual({ kind: 'done', success: false, message: 'bad' })
    expect(interpretInferenceStatus({})).toEqual({ kind: 'lost' })
    bridge(async (method) => {
      if (method === 'host.vault.exists') return { exists: true }
      if (method === 'host.agent.run') throw new Error('agent_unavailable: none installed')
      return {}
    })
    const result = await startMemoryInference({ mode: 'full' })
    expect(result).toMatchObject({ ok: false, reason: 'agent-missing' })
  })
})
