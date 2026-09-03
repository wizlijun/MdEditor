import { invoke } from '@tauri-apps/api/core'
import type { MemorySelection, SearchContextSource } from './session'

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
  memoryContext: (provider: string, model: string | null) =>
    invoke<MemoryContextResult>('smart_search_memory_context', { provider, model }),
  archiveAnswer: (payload: AnswerArchivePayload) =>
    invoke<ArchiveReceipt>('smart_search_archive_answer', { payload }),
  recordFeedback: (answerId: string, value: 'helpful' | 'unhelpful', reason: string | null) =>
    invoke<void>('smart_search_record_feedback', { answerId, value, reason }),
  writeDocument: (payload: DocumentWritePayload) =>
    invoke<ArchiveReceipt>('smart_search_write_document', { payload }),
}
