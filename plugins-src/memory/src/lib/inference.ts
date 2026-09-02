import {
  agentRun,
  agentStatus,
  requestId,
  vaultExists,
  vaultRead,
  vaultWrite,
} from './bridge'
import {
  MEMORY_INFERENCE_STATE,
  MEMORY_INFERENCE_TASK,
  seedMemoryInferenceTask,
} from './inference-task'

export const INFERENCE_POLL_MS = 2000
export type InferenceMode = 'full' | 'incremental'
export type InferenceStart =
  | { ok: true; runId: string; invocationId: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }

export type InferenceRunView =
  | { kind: 'running'; steps: number; last: string }
  | { kind: 'done'; success: boolean; message: string }
  | { kind: 'lost' }

interface InferenceState {
  schema: 'notemd.memory/inference-state/v2'
  invocation_id: string
  last_successful_head: string
  complete: true
}

const seedIo = {
  exists: (path: string) => vaultExists(path).then((value) => value.exists === true),
  read: (path: string) => vaultRead(path).then((value) => value.content),
  write: async (path: string, content: string) => { await vaultWrite(path, content) },
}

function parseState(content: string): InferenceState | null {
  try {
    const value = JSON.parse(content) as Partial<InferenceState>
    return value.schema === 'notemd.memory/inference-state/v2'
      && value.complete === true
      && typeof value.invocation_id === 'string'
      && typeof value.last_successful_head === 'string'
      && value.last_successful_head.length > 0
      ? value as InferenceState
      : null
  } catch {
    return null
  }
}

/** Existing claims are deliberately irrelevant: only a successful scan makes
 * the next run incremental. A hand-authored Memory repository may never have
 * been inferred from the Vault at all. */
export async function detectInferenceMode(): Promise<InferenceMode> {
  const { exists } = await vaultExists(MEMORY_INFERENCE_STATE)
  if (!exists) return 'full'
  const { content } = await vaultRead(MEMORY_INFERENCE_STATE)
  return parseState(content) ? 'incremental' : 'full'
}

export async function completedInference(invocationId: string): Promise<boolean> {
  try {
    const { exists } = await vaultExists(MEMORY_INFERENCE_STATE)
    if (!exists) return false
    const { content } = await vaultRead(MEMORY_INFERENCE_STATE)
    return parseState(content)?.invocation_id === invocationId
  } catch {
    return false
  }
}

export async function startMemoryInference(input: {
  mode: InferenceMode
  harness?: string
}): Promise<InferenceStart> {
  const invocationId = requestId('memory-inference')
  try {
    await seedMemoryInferenceTask(seedIo)
    const { run_id } = await agentRun({
      task: MEMORY_INFERENCE_TASK,
      ...(input.harness ? { harness: input.harness } : {}),
      prompt: [
        `Mode: ${input.mode}`,
        `Invocation-ID: ${invocationId}`,
        `State: ${MEMORY_INFERENCE_STATE}`,
        'Only pending proposals are allowed. Never approve or directly edit Memory authority files.',
      ].join('\n'),
    })
    if (typeof run_id !== 'string' || run_id === '') {
      return { ok: false, reason: 'error', message: 'Agent 未返回 run id。' }
    }
    return { ok: true, runId: run_id, invocationId }
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause)
    return {
      ok: false,
      reason: message.includes('agent_unavailable') ? 'agent-missing' : 'error',
      message,
    }
  }
}

export function interpretInferenceStatus(raw: unknown): InferenceRunView {
  if (!raw || typeof raw !== 'object') return { kind: 'lost' }
  const value = raw as Record<string, unknown>
  if (value.state === 'running') {
    return {
      kind: 'running',
      steps: typeof value.steps === 'number' && Number.isFinite(value.steps) ? value.steps : 0,
      last: typeof value.last === 'string' ? value.last : '',
    }
  }
  if (value.state === 'done' && value.record && typeof value.record === 'object') {
    const record = value.record as Record<string, unknown>
    const success = record.status === 'success'
    const message = success ? record.result : (record.stderr_tail ?? record.result)
    return { kind: 'done', success, message: typeof message === 'string' ? message : '' }
  }
  return { kind: 'lost' }
}

export function memoryInferenceStatus(runId: string, harness?: string): Promise<unknown> {
  return agentStatus(MEMORY_INFERENCE_TASK, runId, harness)
}
