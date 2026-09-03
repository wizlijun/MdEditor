import type { SmartSearchResponse } from '../search/api'

export type SearchPlanMode = 'plan' | 'tune'

export interface ResolvedSearchPlan {
  schemaVersion: number
  intent: { kind: string; focus: string }
  referenceTime: string
  referenceDate: string
  timezone: string
  time: {
    appliesTo: string
    sourceText: string
    after: string | null
    before: string | null
  } | null
  constraints: Record<string, unknown>
  lockedFilters: Record<string, unknown>
  queries: Array<{
    id: string
    logicalId: string
    purpose: string
    terms: string[]
    phrases: string[]
    weight: number
    rationale: string
    filters: Record<string, unknown>
  }>
  sort: 'relevance' | 'doc_date_desc' | 'doc_date_asc'
  unsupportedConstraints: string[]
  ambiguities: string[]
  confidence: 'high' | 'medium' | 'low'
}

export interface PlannedSearchResponse {
  resolvedPlan: ResolvedSearchPlan
  search: SmartSearchResponse
}

export interface SearchPlanTelemetry {
  total: number
  distinctDocuments: number
  truncated: boolean
  subqueries: Array<{
    id: string
    purpose: 'precision' | 'recall' | 'unknown'
    hitCount: number
    executed: boolean
    truncated: boolean
  }>
}

export interface SearchPlanPromptInput {
  mode: SearchPlanMode
  question: string
  referenceTime: string
  timezone: string
  locale: string
  lockedFilters: Record<string, unknown>
  previousPlan?: string
  resolvedPlan?: ResolvedSearchPlan
  telemetry?: SearchPlanTelemetry
}

export function buildSearchPlanPrompt(input: SearchPlanPromptInput): string {
  const {
    mode, question, referenceTime, timezone, locale, lockedFilters,
    previousPlan, resolvedPlan, telemetry,
  } = input
  const tunePacket = mode === 'tune'
    ? [
        '',
        'PREVIOUS_SEARCH_PLAN_JSON',
        previousPlan?.trim() || '{}',
        '',
        'RESOLVED_IMMUTABLE_PLAN_JSON',
        JSON.stringify(resolvedPlan ?? {}),
        '',
        'RETRIEVAL_TELEMETRY_JSON',
        JSON.stringify(telemetry ?? { total: 0, distinctDocuments: 0, truncated: false, subqueries: [] }),
      ]
    : []
  return [
    `MODE: ${mode}`,
    `REFERENCE_TIME: ${referenceTime}`,
    `TIMEZONE: ${timezone}`,
    `LOCALE: ${locale}`,
    `LOCKED_FILTERS_JSON: ${JSON.stringify(lockedFilters)}`,
    '',
    'QUESTION',
    question.trim(),
    ...tunePacket,
    '',
    'Return exactly one SearchPlanV1 JSON object. Do not use Markdown fences and do not answer the question.',
    'Separate document-date constraints from dates that are the subject of the question.',
    'For relative time, emit a bounded expression; the host computes final dates.',
    'Keep search terms concise. Never emit commands, file contents, paths not stated by the user, or tool calls.',
    'In tune mode, preserve every explicit/time/path/type/origin constraint and adjust only terms, phrases, query arms, and weights.',
    '',
    'SCHEMA',
    JSON.stringify({
      schemaVersion: 1,
      intent: { kind: 'answer', focus: 'string' },
      time: null,
      constraints: {
        paths: { anyOf: [], allOf: [] },
        tags: { anyOf: [], allOf: [] },
        types: { anyOf: [] },
        extensions: { anyOf: [] },
        origins: { anyOf: [] },
        linkedPages: { allOf: [] },
      },
      queries: [{
        id: 'q1', purpose: 'precision|recall', terms: [], phrases: [], weight: 1,
        rationale: 'string',
      }],
      sort: 'relevance|doc_date_desc|doc_date_asc',
      unsupportedConstraints: [],
      ambiguities: [],
      confidence: 'high|medium|low',
    }),
    'TIME: use null when there is no time expression; otherwise use {"appliesTo":"document_date|content_date|activity_time|ambiguous","sourceText":"...","expression":<one variant or null>}.',
    'TIME EXPRESSION VARIANTS (use exactly one shape, with no extra fields):',
    '{"kind":"calendar_month","offset":-1}',
    '{"kind":"calendar_week","offset":-1}',
    '{"kind":"quarter","year":2026,"quarter":3}',
    '{"kind":"year","offset":-1}',
    '{"kind":"rolling_window","value":3,"unit":"days|weeks|months|years"}',
    '{"kind":"absolute_range","after":"YYYY-MM-DD","before":"YYYY-MM-DD"}',
  ].join('\n')
}

export function shouldTune(telemetry: SearchPlanTelemetry): boolean {
  if (telemetry.truncated) return false
  const recall = telemetry.subqueries.filter((query) => query.purpose === 'recall')
  const recallComplete = recall.length > 0
    && recall.every((query) => query.executed && !query.truncated)
  if (!recallComplete) return false
  return telemetry.total === 0 || telemetry.distinctDocuments < 3
}
