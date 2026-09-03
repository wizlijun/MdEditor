import { invoke } from '@tauri-apps/api/core'
import type { AgentHarness, AgentOption } from '../agent-picker/types'
import type { PluginManifest } from '../plugins/types'
import type { ModelSelector } from './model-routing'

export const SEARCH_PLAN_TASK = 'search-plan'
export const SEARCH_ANSWER_TASK = 'search-answer'
export const SEARCH_AGENT_POLL_MS = 500

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

export interface AgentTaskStart {
  runId: string
  resolvedModel: string | null
}

export class AgentTaskError extends Error {
  constructor(
    message: string,
    readonly task: string,
    readonly runId: string,
    readonly status: string,
  ) {
    super(message)
    this.name = 'AgentTaskError'
  }
}

export interface AgentPollRetry {
  attempt: number
  maxAttempts: number
  error: string
}

export interface AgentPollOptions {
  transport?: Execute
  intervalMs?: number
  signal?: AbortSignal
  maxTransientErrors?: number
  retryDelayMs?: number
  onRetry?: (retry: AgentPollRetry) => void
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
      harness = normalizeAgentHarness(await execute(manifest.id, 'harness-status', {}))
    } catch {
      harness = null
    }
    return { id: manifest.id, name: manifest.name, harness }
  }))
}

export function normalizeAgentHarness(value: unknown): AgentHarness | null {
  if (!value || typeof value !== 'object') return null
  const raw = value as Record<string, any>
  if (typeof raw.harness !== 'string' || typeof raw.ok !== 'boolean') return null
  const routing = raw.capabilities?.model_routing
  const profiles = routing?.profiles
  const capabilities = raw.capabilities
    && Array.isArray(raw.capabilities.tasks)
    && Array.isArray(raw.capabilities.search_plan_schemas)
    && typeof raw.capabilities.terminal_result === 'boolean'
    && typeof raw.capabilities.input_only_isolation === 'boolean'
    && typeof routing?.invocation_override === 'boolean'
    && Array.isArray(routing?.selectable_models)
    && typeof profiles?.fast?.available === 'boolean'
    && typeof profiles?.default?.available === 'boolean'
      ? raw.capabilities
      : undefined
  return {
    harness: raw.harness,
    ok: raw.ok,
    version: typeof raw.version === 'string' ? raw.version : null,
    origin: typeof raw.origin === 'string' ? raw.origin : undefined,
    default_model: typeof raw.default_model === 'string' ? raw.default_model : null,
    hint: typeof raw.hint === 'string' ? raw.hint : null,
    warning: typeof raw.warning === 'string' ? raw.warning : null,
    capabilities,
  }
}

export async function startSearchAgentTask(
  provider: string,
  prompt: string,
  usageDisplay: 'tip' | 'result' = 'result',
  transport: Execute = execute,
  modelSelector?: ModelSelector,
): Promise<string> {
  return (await startAgentTask(
    provider,
    SEARCH_ANSWER_TASK,
    prompt,
    modelSelector,
    usageDisplay,
    transport,
  )).runId
}

export async function startAgentTask(
  provider: string,
  task: string,
  prompt: string,
  modelSelector?: ModelSelector,
  usageDisplay: 'tip' | 'result' = 'result',
  transport: Execute = execute,
): Promise<AgentTaskStart> {
  const selector = checkedModelSelector(modelSelector)
  const response = await transport(provider, 'run-task', {
    task,
    prompt,
    usage_display: usageDisplay,
    ...selector,
  })
  if (typeof response?.run_id !== 'string' || !response.run_id) {
    throw new Error('the agent provider returned no run id')
  }
  return {
    runId: response.run_id,
    resolvedModel: typeof response.resolved_model === 'string' && response.resolved_model
      ? response.resolved_model
      : null,
  }
}

function checkedModelSelector(selector: ModelSelector | undefined): ModelSelector | undefined {
  if (!selector) return undefined
  const profile = 'model_profile' in selector ? selector.model_profile : undefined
  const model = 'model' in selector ? selector.model : undefined
  if (profile && model) throw new Error('model_profile and model are mutually exclusive')
  if (profile !== undefined && profile !== 'fast' && profile !== 'default') {
    throw new Error(`unknown model profile: ${String(profile)}`)
  }
  if (model !== undefined && !model.trim()) throw new Error('model must not be empty')
  return selector
}

function pause(ms: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return Promise.reject(new DOMException('agent polling aborted', 'AbortError'))
  return new Promise((resolve, reject) => {
    const onAbort = () => {
      clearTimeout(timer)
      reject(new DOMException('agent polling aborted', 'AbortError'))
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}

export async function pollSearchAgentTask(
  provider: string,
  runId: string,
  onProgress: (progress: AgentProgress) => void,
  options: AgentPollOptions = {},
): Promise<AgentTaskResult> {
  return pollAgentTask(provider, SEARCH_ANSWER_TASK, runId, onProgress, options)
}

export async function pollAgentTask(
  provider: string,
  task: string,
  runId: string,
  onProgress: (progress: AgentProgress) => void,
  options: AgentPollOptions = {},
): Promise<AgentTaskResult> {
  const transport = options.transport ?? execute
  const intervalMs = options.intervalMs ?? SEARCH_AGENT_POLL_MS
  const maxTransientErrors = options.maxTransientErrors ?? 2
  const retryDelayMs = options.retryDelayMs ?? 250
  let transientErrors = 0
  while (!options.signal?.aborted) {
    let response: any
    try {
      response = await transport(provider, 'run-status', {
        task,
        run_id: runId,
      })
      transientErrors = 0
    } catch (error) {
      transientErrors += 1
      const message = error instanceof Error ? error.message : String(error)
      if (transientErrors > maxTransientErrors) {
        throw new AgentTaskError(message, task, runId, 'transport_error')
      }
      options.onRetry?.({
        attempt: transientErrors,
        maxAttempts: maxTransientErrors,
        error: message,
      })
      await pause(retryDelayMs * transientErrors, options.signal)
      continue
    }
    if (response?.state === 'running') {
      onProgress({ steps: Number(response.steps ?? 0), last: String(response.last ?? '') })
      await pause(intervalMs, options.signal)
      continue
    }
    if (response?.state !== 'done') {
      throw new AgentTaskError('the agent run ended without a record', task, runId, String(response?.state ?? 'lost'))
    }
    const record = response.record ?? {}
    const status = String(record.status ?? 'error')
    if (task === SEARCH_PLAN_TASK) {
      if (status !== 'success') {
        throw new AgentTaskError(
          String(response.terminal_result?.content ?? record.stderr_tail ?? `planner run failed: ${status}`),
          task,
          runId,
          status,
        )
      }
      if (response.terminal_result?.complete !== true) {
        throw new AgentTaskError('planner returned no complete terminal result', task, runId, 'incomplete')
      }
      const content = String(response.terminal_result.content ?? '')
      if (!content.trim()) {
        throw new AgentTaskError('planner returned an empty terminal result', task, runId, 'empty')
      }
      return { runId, status, content, usage: record.usage ?? null }
    }
    const complete = response.terminal_result?.complete === true
      ? String(response.terminal_result.content ?? '')
      : String(record.result ?? record.stderr_tail ?? '')
    if (status !== 'success' && status !== 'skipped') {
      throw new AgentTaskError(complete || String(record.stderr_tail ?? `agent run failed: ${status}`), task, runId, status)
    }
    return { runId, status, content: complete, usage: record.usage ?? null }
  }
  throw new DOMException('agent polling aborted', 'AbortError')
}
