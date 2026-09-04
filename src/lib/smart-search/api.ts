import { invoke } from '@tauri-apps/api/core'
import type { PlannedSearchResponse } from './plan'
import type { SummaryTaskStart } from './summary'
import type { SummaryStyle } from './settings'
import type { ModelSelector } from './model-routing'
import type { HandoffPacket } from './handoff'
import type { AgentTaskStart } from './agent'

export const smartSearchApi = {
  planContext: (originalQuery: string, referenceTime: string, timezone: string) => invoke<{
    lockedFilters: Record<string, unknown>
    referenceDate: string
    timeAnchors: Record<string, { after: string; before: string }>
  }>(
    'notemd_search_plan_context', { originalQuery, referenceTime, timezone },
  ),
  plannedSearch: (
    originalQuery: string,
    plan: unknown,
    referenceTime: string,
    timezone: string,
    options: {
      limit?: number
      deep?: boolean
      timeoutMs?: number
      retainRun?: boolean
    } = {},
  ) => invoke<PlannedSearchResponse>('notemd_planned_search', {
    originalQuery,
    plan,
    referenceTime,
    timezone,
    ...options,
  }),
  startSummary: (
    lookupRunId: string,
    selectedResultIds: string[],
    sourceLimit: number,
    charLimit: number,
    style: SummaryStyle,
    provider: string,
    modelSelector: ModelSelector,
    invocationId: string,
  ) => invoke<SummaryTaskStart>('smart_lookup_start_summary', {
    lookupRunId,
    selectedResultIds,
    sourceLimit,
    charLimit,
    style,
    provider,
    invocationId,
    ...modelSelector,
  }),
  startHandoff: (
    packet: HandoffPacket,
    provider: string,
    invocationId: string,
  ) => invoke<AgentTaskStart>('smart_lookup_start_handoff', {
    question: packet.question,
    resolvedFilters: packet.resolvedFilters,
    queryTerms: packet.queryTerms,
    selectedRefs: packet.selectedRefs,
    provider,
    invocationId,
  }),
}
