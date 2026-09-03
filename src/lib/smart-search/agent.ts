import { invoke } from '@tauri-apps/api/core'
import type { AgentHarness, AgentOption } from '../agent-picker/types'
import type { PluginManifest } from '../plugins/types'

export const SEARCH_ANSWER_TASK = 'search-answer'
export const SEARCH_AGENT_POLL_MS = 1_000

export interface AgentProgress {
  steps: number
  last: string
}

export interface AgentTaskResult {
  runId: string
  status: string
  content: string
  usage: unknown
}

type Execute = (provider: string, command: string, context: Record<string, unknown>) => Promise<any>

const execute: Execute = (provider, command, context) => invoke('plugin_v2_execute', {
  pluginId: provider,
  command,
  context,
})

export async function loadSearchAgentOptions(): Promise<AgentOption[]> {
  const manifests = await invoke<PluginManifest[]>('get_plugin_manifests')
  const providers = manifests.filter((manifest) => manifest.agent_provider === true)
  return await Promise.all(providers.map(async (manifest) => {
    let harness: AgentHarness | null = null
    try {
      harness = await execute(manifest.id, 'harness-status', {}) as AgentHarness
    } catch {
      harness = null
    }
    return { id: manifest.id, name: manifest.name, harness }
  }))
}

export async function startSearchAgentTask(
  provider: string,
  prompt: string,
  usageDisplay: 'tip' | 'result' = 'result',
  transport: Execute = execute,
): Promise<string> {
  const response = await transport(provider, 'run-task', {
    task: SEARCH_ANSWER_TASK,
    prompt,
    usage_display: usageDisplay,
  })
  if (typeof response?.run_id !== 'string' || !response.run_id) {
    throw new Error('the agent provider returned no run id')
  }
  return response.run_id
}

function pause(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export async function pollSearchAgentTask(
  provider: string,
  runId: string,
  onProgress: (progress: AgentProgress) => void,
  options: { transport?: Execute; intervalMs?: number; signal?: AbortSignal } = {},
): Promise<AgentTaskResult> {
  const transport = options.transport ?? execute
  const intervalMs = options.intervalMs ?? SEARCH_AGENT_POLL_MS
  while (!options.signal?.aborted) {
    const response = await transport(provider, 'run-status', {
      task: SEARCH_ANSWER_TASK,
      run_id: runId,
    })
    if (response?.state === 'running') {
      onProgress({ steps: Number(response.steps ?? 0), last: String(response.last ?? '') })
      await pause(intervalMs)
      continue
    }
    if (response?.state !== 'done') throw new Error('the agent run ended without a record')
    const record = response.record ?? {}
    const status = String(record.status ?? 'error')
    const complete = response.terminal_result?.complete === true
      ? String(response.terminal_result.content ?? '')
      : String(record.result ?? record.stderr_tail ?? '')
    if (status !== 'success' && status !== 'skipped') {
      throw new Error(complete || `agent run failed: ${status}`)
    }
    return { runId, status, content: complete, usage: record.usage ?? null }
  }
  throw new DOMException('agent polling aborted', 'AbortError')
}
