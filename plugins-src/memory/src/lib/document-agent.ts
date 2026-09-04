import { agentRun, agentStatus } from './bridge'
import type { AgentOption } from './agent-picker/types'

export const DOCUMENT_AGENT_TASK = 'governed-document-review'
export const DOCUMENT_AGENT_POLL_MS = 2000

export interface DocumentAgentInput {
  action: 'suggest' | 'assess'
  documentId: string
  blockId: string
  blockRevision: string
  content: string
  instruction: string
}

export type DocumentAgentResult =
  | {
      schema: 'notemd.cdr/agent-result/v1'
      kind: 'suggestion'
      content: string
      summary: string
    }
  | {
      schema: 'notemd.cdr/agent-result/v1'
      kind: 'assessment'
      conclusion: 'verified' | 'needs-review'
      summary: string
    }

export type DocumentAgentStart =
  | { ok: true; runId: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }

export type DocumentAgentRunView =
  | { kind: 'running'; steps: number; last: string }
  | { kind: 'done'; success: true; result: DocumentAgentResult; providerId: string }
  | { kind: 'done'; success: false; message: string }
  | { kind: 'lost' }

export type DocumentAgentReadiness =
  | { ok: true; providerId: string; label: string }
  | { ok: false; message: string }

export function documentAgentReadiness(provider?: AgentOption): DocumentAgentReadiness {
  if (!provider) return { ok: false, message: '请选择一个 Agent。' }
  if (provider.harness?.ok !== true) return { ok: false, message: `${provider.name} 当前不可用。` }
  const capabilities = provider.harness.capabilities
  if (!capabilities
    || capabilities.input_only_isolation !== true
    || capabilities.terminal_result !== true
    || !capabilities.tasks.includes(DOCUMENT_AGENT_TASK)) {
    return { ok: false, message: `${provider.name} 需要升级后才能安全处理共写文档。` }
  }
  return { ok: true, providerId: provider.id, label: provider.name }
}

export async function startDocumentAgent(
  input: DocumentAgentInput,
  harness: string,
): Promise<DocumentAgentStart> {
  if (!harness.trim()) return { ok: false, reason: 'agent-missing', message: '未选择 Agent provider。' }
  try {
    const { run_id } = await agentRun({
      task: DOCUMENT_AGENT_TASK,
      harness,
      prompt: `Input:\n${JSON.stringify(input)}`,
    })
    if (typeof run_id !== 'string' || !run_id) {
      return { ok: false, reason: 'error', message: 'Agent 未返回 run id。' }
    }
    return { ok: true, runId: run_id }
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause)
    return {
      ok: false,
      reason: message.includes('agent_unavailable') ? 'agent-missing' : 'error',
      message,
    }
  }
}

function object(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function exactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const actual = Object.keys(value).sort()
  return actual.length === expected.length && expected.slice().sort().every((key, index) => key === actual[index])
}

function boundedString(value: unknown, max: number): string | null {
  return typeof value === 'string' && value.trim() && value.length <= max ? value : null
}

export function parseDocumentAgentResult(content: string): DocumentAgentResult {
  let raw: unknown
  try {
    raw = JSON.parse(content)
  } catch {
    throw new Error('Agent 没有返回有效的 JSON 结果。')
  }
  const value = object(raw)
  if (!value || value.schema !== 'notemd.cdr/agent-result/v1') {
    throw new Error('Agent 返回了不支持的结果格式。')
  }
  const summary = boundedString(value.summary, 2000)
  if (!summary) throw new Error('Agent 结果缺少有效摘要。')
  if (value.kind === 'suggestion') {
    if (!exactKeys(value, ['schema', 'kind', 'content', 'summary'])) {
      throw new Error('Agent 建议包含未声明字段。')
    }
    const suggested = boundedString(value.content, 64 * 1024)
    if (!suggested) throw new Error('Agent 建议正文无效。')
    return { schema: value.schema, kind: value.kind, content: suggested, summary }
  }
  if (value.kind === 'assessment') {
    if (!exactKeys(value, ['schema', 'kind', 'conclusion', 'summary'])) {
      throw new Error('Agent 检查结果包含未声明字段。')
    }
    if (value.conclusion !== 'verified' && value.conclusion !== 'needs-review') {
      throw new Error('Agent 检查结论无效。')
    }
    return { schema: value.schema, kind: value.kind, conclusion: value.conclusion, summary }
  }
  throw new Error('Agent 返回了未知结果类型。')
}

export function interpretDocumentAgentStatus(raw: unknown, requestedHarness: string): DocumentAgentRunView {
  const value = object(raw)
  if (!value) return { kind: 'lost' }
  if (value.state === 'running') {
    return {
      kind: 'running',
      steps: typeof value.steps === 'number' && Number.isFinite(value.steps) ? value.steps : 0,
      last: typeof value.last === 'string' ? value.last : '',
    }
  }
  if (value.state !== 'done') return { kind: 'lost' }
  const record = object(value.record)
  if (!record) return { kind: 'lost' }
  if (record.status !== 'success') {
    const message = typeof record.stderr_tail === 'string' && record.stderr_tail
      ? record.stderr_tail
      : typeof record.result === 'string' ? record.result : 'Agent 运行失败。'
    return { kind: 'done', success: false, message }
  }
  const terminal = object(value.terminal_result)
  if (!terminal || terminal.complete !== true || typeof terminal.content !== 'string') {
    return { kind: 'done', success: false, message: 'Agent 未提供可校验的完整结果。' }
  }
  try {
    return {
      kind: 'done',
      success: true,
      result: parseDocumentAgentResult(terminal.content),
      providerId: requestedHarness,
    }
  } catch (cause) {
    return {
      kind: 'done',
      success: false,
      message: cause instanceof Error ? cause.message : String(cause),
    }
  }
}

export function documentAgentStatus(runId: string, harness: string): Promise<unknown> {
  return agentStatus(DOCUMENT_AGENT_TASK, runId, harness)
}
