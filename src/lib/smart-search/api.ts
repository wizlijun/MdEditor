import { invoke } from '@tauri-apps/api/core'
import type { MemorySelection, SearchContextSource } from './session'
import type { PlannedSearchResponse } from './plan'

export interface MemoryContextResult {
  available: boolean
  selected: MemorySelection[]
  excludedSummary: Record<string, number>
  manifestId: string | null
  error: string | null
}

export interface AnswerArchivePayload {
  answerId: string
  query: string
  answer: string
  provider: string
  model: string | null
  runId: string
  memoryManifestId: string | null
  sources: SearchContextSource[]
}

export interface ArchiveReceipt {
  path: string
  created: boolean
}

export interface DocumentWritePayload {
  title: string
  query: string
  content: string
  provider: string
  model: string | null
  runId: string
  memoryManifestId: string | null
  sources: SearchContextSource[]
}

export const smartSearchApi = {
  planContext: (originalQuery: string) => invoke<{ lockedFilters: Record<string, unknown> }>(
    'notemd_search_plan_context', { originalQuery },
  ),
  plannedSearch: (
    originalQuery: string,
    plan: unknown,
    referenceTime: string,
    timezone: string,
    options: { limit?: number; deep?: boolean; timeoutMs?: number; baselinePlan?: unknown } = {},
  ) => invoke<PlannedSearchResponse>('notemd_planned_search', {
    originalQuery,
    plan,
    referenceTime,
    timezone,
    ...options,
  }),
  freezeSources: (sources: SearchContextSource[]) =>
    invoke<SearchContextSource[]>('smart_search_freeze_sources', { sources }),
  memoryContext: (provider: string, model: string | null) =>
    invoke<MemoryContextResult>('smart_search_memory_context', { provider, model }),
  archiveAnswer: (payload: AnswerArchivePayload) =>
    invoke<ArchiveReceipt>('smart_search_archive_answer', { payload }),
  recordFeedback: (answerId: string, value: 'helpful' | 'unhelpful', reason: string | null) =>
    invoke<void>('smart_search_record_feedback', { answerId, value, reason }),
  writeDocument: (payload: DocumentWritePayload) =>
    invoke<ArchiveReceipt>('smart_search_write_document', { payload }),
}
