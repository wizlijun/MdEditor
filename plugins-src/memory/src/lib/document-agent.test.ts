// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  DOCUMENT_AGENT_TASK,
  documentAgentReadiness,
  documentAgentStatus,
  interpretDocumentAgentStatus,
  parseDocumentAgentResult,
  startDocumentAgent,
} from './document-agent'

afterEach(() => { vi.restoreAllMocks() })

function installBridge(handler: (method: string, params?: any) => Promise<any>) {
  const request = vi.fn(handler)
  window.notemd = { pluginId: 'notemd.memory', locale: 'zh', theme: 'system', request, onMessage: () => {} }
  return request
}

const input = {
  action: 'suggest' as const,
  documentId: 'document-1',
  blockId: 'block-1',
  blockRevision: 'block-1/1',
  content: '原文',
  instruction: '写得更清楚',
}

describe('governed document Agent adapter', () => {
  it('starts only the provider selected by the caller without writing task files', async () => {
    const request = installBridge(async (method) => {
      if (method === 'host.agent.run') return { run_id: 'run-1' }
      return {}
    })

    await expect(startDocumentAgent(input, 'notemd.codex-agent')).resolves.toEqual({ ok: true, runId: 'run-1' })
    const methods = request.mock.calls.map(([method]) => method)
    expect(methods.some((method) => method.startsWith('host.vault.'))).toBe(false)
    const params = request.mock.calls.find(([method]) => method === 'host.agent.run')?.[1]
    expect(params).toMatchObject({ task: DOCUMENT_AGENT_TASK, harness: 'notemd.codex-agent' })
    expect(params.prompt).toContain(JSON.stringify(input))
  })

  it('fails readiness closed unless one healthy provider advertises the exact isolated task', () => {
    const base = { id: 'notemd.codex-agent', name: 'Codex Agent' }
    expect(documentAgentReadiness()).toMatchObject({ ok: false })
    expect(documentAgentReadiness({ ...base, harness: { harness: 'Codex', ok: false } })).toMatchObject({ ok: false })
    expect(documentAgentReadiness({
      ...base,
      harness: {
        harness: 'Codex', ok: true,
        capabilities: {
          tasks: ['search-answer'], search_plan_schemas: [1], terminal_result: true,
          input_only_isolation: true,
          model_routing: { invocation_override: true, profiles: { fast: { available: true }, default: { available: true } }, selectable_models: [] },
        },
      },
    })).toMatchObject({ ok: false })
    expect(documentAgentReadiness({
      ...base,
      harness: {
        harness: 'Codex', ok: true,
        capabilities: {
          tasks: [DOCUMENT_AGENT_TASK], search_plan_schemas: [1], terminal_result: true,
          input_only_isolation: true,
          model_routing: { invocation_override: true, profiles: { fast: { available: true }, default: { available: true } }, selectable_models: [] },
        },
      },
    })).toEqual({ ok: true, providerId: 'notemd.codex-agent', label: 'Codex Agent' })
  })

  it('strictly parses suggestions and assessments', () => {
    expect(parseDocumentAgentResult(JSON.stringify({
      schema: 'notemd.cdr/agent-result/v1', kind: 'suggestion', content: '改写', summary: '更清楚',
    }))).toMatchObject({ kind: 'suggestion', content: '改写' })
    expect(parseDocumentAgentResult(JSON.stringify({
      schema: 'notemd.cdr/agent-result/v1', kind: 'assessment', conclusion: 'needs-review', summary: '缺少依据',
    }))).toMatchObject({ kind: 'assessment', conclusion: 'needs-review' })
    expect(() => parseDocumentAgentResult('```json\n{}\n```')).toThrow('有效的 JSON')
    expect(() => parseDocumentAgentResult(JSON.stringify({
      schema: 'notemd.cdr/agent-result/v1', kind: 'suggestion', content: '改写', summary: '原因', extra: true,
    }))).toThrow('未声明字段')
  })

  it('requires the complete machine result and keeps the Host-selected provider identity', () => {
    expect(interpretDocumentAgentStatus({ state: 'running', steps: 2, last: 'reviewing' }, 'notemd.codex-agent')).toEqual({
      kind: 'running', steps: 2, last: 'reviewing',
    })
    expect(interpretDocumentAgentStatus({
      state: 'done',
      record: { status: 'success', result: 'truncated', harness: 'notemd.deepseek-agent' },
      terminal_result: {
        complete: true,
        content: JSON.stringify({
          schema: 'notemd.cdr/agent-result/v1', kind: 'suggestion', content: '完整改写', summary: '原因',
        }),
      },
    }, 'notemd.codex-agent')).toEqual({
      kind: 'done', success: true, providerId: 'notemd.codex-agent',
      result: { schema: 'notemd.cdr/agent-result/v1', kind: 'suggestion', content: '完整改写', summary: '原因' },
    })
    expect(interpretDocumentAgentStatus({
      state: 'done', record: { status: 'success', result: '{partial' },
    }, 'notemd.codex-agent')).toEqual({ kind: 'done', success: false, message: 'Agent 未提供可校验的完整结果。' })
  })

  it('routes status to the provider that started the run', async () => {
    const request = installBridge(async () => ({ state: 'lost' }))
    await documentAgentStatus('run-3', 'notemd.claude-agent')
    expect(request).toHaveBeenCalledWith('host.agent.status', {
      task: DOCUMENT_AGENT_TASK,
      run_id: 'run-3',
      harness: 'notemd.claude-agent',
    })
  })

  it('reports an unavailable provider without throwing', async () => {
    installBridge(async (method) => {
      if (method === 'host.vault.exists') return { exists: true }
      if (method === 'host.agent.run') throw new Error('agent_unavailable: none installed')
      return {}
    })
    await expect(startDocumentAgent(input, 'notemd.codex-agent')).resolves.toMatchObject({ ok: false, reason: 'agent-missing' })
  })
})
