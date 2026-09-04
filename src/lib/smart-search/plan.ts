import type { SmartSearchResponse } from '../search/api'

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
  lookupRunId?: string
}

export interface SearchPlanPromptInput {
  question: string
  referenceTime: string
  timezone: string
  locale: string
  lockedFilters: Record<string, unknown>
}

export function buildSearchPlanPrompt(input: SearchPlanPromptInput): string {
  const { question, referenceTime, timezone, locale, lockedFilters } = input
  return [
    'MODE: plan',
    `REFERENCE_TIME: ${referenceTime}`,
    `TIMEZONE: ${timezone}`,
    `LOCALE: ${locale}`,
    `LOCKED_FILTERS_JSON: ${JSON.stringify(lockedFilters)}`,
    '',
    'QUESTION',
    question.trim(),
    '',
    'Return exactly one SearchPlanV1 JSON object. Do not use Markdown fences and do not answer the question.',
    'Assess a document-date window before choosing search terms or query arms, even when the question gives no exact date.',
    'When the intent clearly concerns current status, recent progress, or the latest work cycle, estimate a conservative bounded document_date window. Prefer a broad recall-oriented window over a narrow guess.',
    'Never apply one default recent window to every question. If time evidence is weak, materially different windows are plausible, or the question is timeless, definitional, identity-related, or historical, keep time null.',
    'Separate document-date constraints from dates that are the subject of the question.',
    'When time applies to document_date, put it only in time.expression; do not copy time.sourceText into terms or phrases.',
    'For activity_time or ambiguous time, record the limitation and never substitute a document-date filter.',
    'For relative time, emit a bounded expression; the host computes final dates.',
    'Keep search terms concise. Never emit commands, file contents, paths not stated by the user, or tool calls.',
    'Emit at most two logical query arms: normally one precision and optionally one recall arm.',
    '',
    'SCHEMA',
    JSON.stringify({
      schemaVersion: 1,
      intent: { kind: 'answer|locate|list|summarize|compare', focus: 'string' },
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
