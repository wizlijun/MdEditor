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
  referenceDate: string
  timezone: string
  locale: string
  lockedFilters: Record<string, unknown>
  timeAnchors: Record<string, { after: string; before: string }>
}

export function buildSearchPlanPrompt(input: SearchPlanPromptInput): string {
  const {
    question, referenceTime, referenceDate, timezone, locale, lockedFilters, timeAnchors,
  } = input
  return [
    'MODE: plan',
    `REFERENCE_TIME: ${referenceTime}`,
    `REFERENCE_DATE: ${referenceDate}`,
    `TIMEZONE: ${timezone}`,
    `LOCALE: ${locale}`,
    `LOCKED_FILTERS_JSON: ${JSON.stringify(lockedFilters)}`,
    `TRUSTED_TIME_ANCHORS_JSON: ${JSON.stringify(timeAnchors)}`,
    '',
    'QUESTION',
    question.trim(),
    '',
    'Return exactly one SearchPlanV1 JSON object. Do not use Markdown fences and do not answer the question.',
    'TIME GATE — finish these steps before choosing any search term or query arm:',
    '1. Inspect every temporal mention in QUESTION before retrieval planning. Quote the smallest exact source span for the document-date window and classify whether each other mention constrains document_date, content_date, activity_time, or is ambiguous.',
    '2. If it constrains document_date, choose one bounded expression before planning retrieval. Use time: null only after deciding there is no usable time evidence.',
    '3. Remove a document_date sourceText span from the text used to create terms and phrases.',
    'For document_date, sourceText must be an exact substring of QUESTION; the host rejects invented or paraphrased evidence.',
    'Do not infer a date window when QUESTION has no explicit temporal cue. A cue may be relative, such as today, recently, current, latest, this week, or last quarter.',
    'Never apply one default recent window to every question. If a temporal cue is weak or materially different windows are plausible, classify it as ambiguous. If the question has no temporal evidence or is timeless, definitional, identity-related, or historical, keep time null.',
    'Separate document-date constraints from dates that are the subject of the question.',
    'If several temporal mentions cannot be represented as one document-date window, use ambiguous instead of silently choosing one. If another mention is a content date, preserve it in a topical term or phrase.',
    'When time applies to document_date, put it only in time.expression; do not copy time.sourceText into terms or phrases.',
    'For activity_time or ambiguous time, record the limitation and never substitute a document-date filter.',
    'TRUSTED_TIME_ANCHORS_JSON is computed by the host from REFERENCE_TIME and TIMEZONE. Use calendar_week/calendar_month/year for matching relative week/month/year cues. For relative day or quarter cues, copy the matching anchor exact after/before pair into absolute_range. The host rejects a range that conflicts with a recognized cue; never calculate anchor dates yourself.',
    'For an explicit quarter such as 2026 Q3, use quarter. For a rolling duration such as the last 7 days, copy its exact number and unit into rolling_window. For one complete explicit date, use absolute_range with after and before equal; for a complete explicit date interval, use its exact inclusive endpoints. The host validates both forms against sourceText.',
    'rolling_window is inclusive: N days covers exactly N calendar dates including today, and N weeks covers exactly N×7 dates including today.',
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
