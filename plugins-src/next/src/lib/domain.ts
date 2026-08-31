import {
  DEFAULT_WAITING_WARNING,
  DEFAULT_WIP_LIMIT,
  NEXT_ACTIONS,
  type AppendOptions,
  type AppendValidation,
  type DomainIssue,
  type EventValidation,
  type FieldValidationError,
  type IdeaProjection,
  type IdeaState,
  type LedgerProjection,
  type NextAction,
  type NextEvent,
  type SourceRef,
  type UnsupportedEvent,
  type UnsupportedIdea,
} from './model'

type UnknownRecord = Record<string, unknown>

interface MutableProjection {
  ideas: Map<string, IdeaProjection>
  eventById: Map<string, unknown>
  sourceToIdeaId: Map<string, string>
  issues: DomainIssue[]
  idempotentEventIds: string[]
}

function isRecord(value: unknown): value is UnknownRecord {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function isNonBlankString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0
}

function isDateLike(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?(?:Z|[+-]\d{2}:\d{2})?)?$/.exec(value)
  if (!match || Number.isNaN(Date.parse(value))) return false
  const calendarDate = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])))
  return calendarDate.toISOString().slice(0, 10) === `${match[1]}-${match[2]}-${match[3]}`
}

function isTimestamp(value: string): boolean {
  return isDateLike(value) && /T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?(?:Z|[+-]\d{2}:\d{2})$/.test(value)
}

export function isNextAction(value: unknown): value is NextAction {
  return typeof value === 'string' && (NEXT_ACTIONS as readonly string[]).includes(value)
}

/**
 * Source identity is path + created when created exists. Old documents without
 * created deliberately remain path-only; callers must never use that to guess a
 * rename. A relink is an explicit event with a user-selected path.
 */
export function sourceKey(source: SourceRef): string {
  return JSON.stringify([source.path, source.created ?? null])
}

function validateSource(value: unknown, field: string, errors: FieldValidationError[]): value is SourceRef {
  if (!isRecord(value)) {
    errors.push({ field, message: 'must be an object' })
    return false
  }
  let valid = true
  if (!isNonBlankString(value.path)) {
    errors.push({ field: `${field}.path`, message: 'must be a non-blank string' })
    valid = false
  }
  if (value.created !== undefined && !isNonBlankString(value.created)) {
    errors.push({ field: `${field}.created`, message: 'must be a non-blank string when present' })
    valid = false
  }
  return valid
}

function requireString(record: UnknownRecord, field: string, errors: FieldValidationError[]): void {
  if (!isNonBlankString(record[field])) {
    errors.push({ field, message: 'must be a non-blank string' })
  }
}

function optionalString(record: UnknownRecord, field: string, errors: FieldValidationError[]): void {
  if (record[field] !== undefined && !isNonBlankString(record[field])) {
    errors.push({ field, message: 'must be a non-blank string when present' })
  }
}

function requireDate(
  record: UnknownRecord,
  field: string,
  errors: FieldValidationError[],
  timestamp = false,
): void {
  requireString(record, field, errors)
  const value = record[field]
  if (isNonBlankString(value) && !(timestamp ? isTimestamp(value) : isDateLike(value))) {
    errors.push({ field, message: timestamp ? 'must be an ISO timestamp with timezone' : 'must be an ISO date or timestamp' })
  }
}

function validateExit(record: UnknownRecord, errors: FieldValidationError[]): void {
  const value = record.exit
  if (!isRecord(value)) {
    errors.push({ field: 'exit', message: 'must be an object' })
    return
  }
  if (!isNonBlankString(value.kind)) {
    errors.push({ field: 'exit.kind', message: 'must be a non-blank string' })
    return
  }

  optionalString(record, 'reason', errors)
  optionalString(record, 'target', errors)
  optionalString(record, 'result', errors)

  switch (value.kind) {
    case 'done':
      if (value.via !== undefined && value.via !== 'delegate') {
        errors.push({ field: 'exit.via', message: 'done only permits via=delegate' })
      }
      break
    case 'stopped':
      if (!['drop', 'disproved', 'ignore'].includes(String(value.via))) {
        errors.push({ field: 'exit.via', message: 'stopped requires drop, disproved, or ignore' })
      }
      if ((value.via === 'drop' || value.via === 'disproved') && !isNonBlankString(record.reason)) {
        errors.push({ field: 'reason', message: `${value.via} requires a reason` })
      }
      break
    case 'transferred':
      if (!['merge', 'project', 'delegate', 'buy', 'publish'].includes(String(value.via))) {
        errors.push({ field: 'exit.via', message: 'transferred requires a supported destination kind' })
      }
      if (!isNonBlankString(record.target)) {
        errors.push({ field: 'target', message: 'transferred requires a target' })
      }
      break
    case 'compressed':
      if (!['principle', 'automate'].includes(String(value.via))) {
        errors.push({ field: 'exit.via', message: 'compressed requires principle or automate' })
      }
      if (!isNonBlankString(record.target)) {
        errors.push({ field: 'target', message: 'compressed requires a target' })
      }
      break
    default:
      errors.push({ field: 'exit.kind', message: `unsupported exit kind: ${value.kind}` })
  }
}

/** Validate one event without discarding fields this v1 reader does not own. */
export function validateEvent(value: unknown): EventValidation {
  if (!isRecord(value)) {
    return { ok: false, errors: [{ field: '$', message: 'event must be an object' }] }
  }

  const errors: FieldValidationError[] = []
  requireDate(value, 'at', errors, true)
  requireString(value, 'event_id', errors)
  requireString(value, 'idea_id', errors)
  requireString(value, 'action', errors)
  if (value.source !== undefined) validateSource(value.source, 'source', errors)

  if (errors.length > 0) return { ok: false, errors }
  if (!isNextAction(value.action)) {
    return { ok: true, known: false, event: value as UnsupportedEvent }
  }

  switch (value.action) {
    case 'commit':
      requireString(value, 'commitment', errors)
      requireString(value, 'next_action', errors)
      requireString(value, 'close_condition', errors)
      break
    case 'wait':
      requireString(value, 'waiting_for', errors)
      requireDate(value, 'review_at', errors)
      break
    case 'park':
      requireString(value, 'wake_trigger', errors)
      optionalString(value, 'next_action', errors)
      break
    case 'settle':
      validateExit(value, errors)
      break
    case 'relink':
      if (value.source === undefined) {
        errors.push({ field: 'source', message: 'relink requires an explicit source' })
      }
      break
    case 'reopen':
      break
  }

  if (errors.length > 0) return { ok: false, errors }
  return { ok: true, known: true, event: value as unknown as NextEvent }
}

function clonePayload<T>(value: T): T {
  if (Array.isArray(value)) return value.map((item) => clonePayload(item)) as T
  if (!isRecord(value)) return value
  const copy: UnknownRecord = {}
  for (const [key, item] of Object.entries(value)) copy[key] = clonePayload(item)
  return copy as T
}

function payloadEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) return false
    return left.every((item, index) => payloadEqual(item, right[index]))
  }
  if (!isRecord(left) || !isRecord(right)) return false
  const leftKeys = Object.keys(left).sort()
  const rightKeys = Object.keys(right).sort()
  if (leftKeys.length !== rightKeys.length) return false
  return leftKeys.every((key, index) => key === rightKeys[index] && payloadEqual(left[key], right[key]))
}

function envelopeString(value: unknown, field: string): string | undefined {
  return isRecord(value) && isNonBlankString(value[field]) ? value[field] : undefined
}

function envelopeSource(value: unknown): SourceRef | undefined {
  if (!isRecord(value) || value.source === undefined) return undefined
  const errors: FieldValidationError[] = []
  return validateSource(value.source, 'source', errors) ? value.source : undefined
}

function addIssue(state: MutableProjection, issue: DomainIssue): void {
  state.issues.push(issue)
}

function unknownActionsOf(idea: IdeaProjection | undefined): string[] {
  return idea?.state === 'unsupported' ? [...idea.unsupported_actions] : []
}

function markUnsupported(
  state: MutableProjection,
  ideaId: string,
  eventId: string,
  at: string,
  source?: SourceRef,
  action?: string,
): void {
  const current = state.ideas.get(ideaId)
  const actions = unknownActionsOf(current)
  if (action && !actions.includes(action)) actions.push(action)
  const lastKnownState = current?.state === 'unsupported'
    ? current.last_known_state
    : current?.state
  const unsupported: UnsupportedIdea = {
    idea_id: ideaId,
    state: 'unsupported',
    last_event_id: eventId,
    last_at: at,
    unsupported_actions: actions,
    ...(current?.project ? { project: current.project } : {}),
    ...(current?.source ? { source: current.source } : source ? { source } : {}),
    ...(lastKnownState ? { last_known_state: lastKnownState } : {}),
  }
  state.ideas.set(ideaId, unsupported)
}

function issueForInvalidEvent(value: unknown, index: number, errors: readonly FieldValidationError[]): DomainIssue {
  return {
    code: 'invalid_event',
    severity: 'blocking',
    message: errors.map((error) => `${error.field}: ${error.message}`).join('; '),
    ...(envelopeString(value, 'event_id') ? { event_id: envelopeString(value, 'event_id') } : {}),
    ...(envelopeString(value, 'idea_id') ? { idea_id: envelopeString(value, 'idea_id') } : {}),
    event_index: index,
    fields: errors.map((error) => error.field),
  }
}

function sourceMatches(left: SourceRef, right: SourceRef): boolean {
  return sourceKey(left) === sourceKey(right)
}

function claimedSource(key: string): SourceRef | null {
  try {
    const parsed = JSON.parse(key) as unknown
    if (!Array.isArray(parsed) || parsed.length !== 2 || !isNonBlankString(parsed[0])) return null
    if (parsed[1] !== null && !isNonBlankString(parsed[1])) return null
    return { path: parsed[0], ...(parsed[1] ? { created: parsed[1] } : {}) }
  } catch {
    return null
  }
}

/**
 * Distinct creation markers can prove that a reused path names a new source.
 * If either side lacks created, the two claims remain ambiguous and conflict.
 */
export function ownersOfSource(claims: ReadonlyMap<string, string>, source: SourceRef): ReadonlySet<string> {
  const owners = new Set<string>()
  for (const [key, owner] of claims) {
    const claimed = claimedSource(key)
    if (claimed?.path !== source.path) continue
    if (
      claimed.created === source.created
      || claimed.created === undefined
      || source.created === undefined
    ) owners.add(owner)
  }
  return owners
}

function claimSource(
  state: MutableProjection,
  source: SourceRef,
  ideaId: string,
  eventId: string,
  at: string,
  index: number,
): boolean {
  const key = sourceKey(source)
  const owners = ownersOfSource(state.sourceToIdeaId, source)
  const conflictingOwners = [...owners].filter((owner) => owner !== ideaId)
  if (conflictingOwners.length === 0) {
    state.sourceToIdeaId.set(key, ideaId)
    return true
  }
  const owner = conflictingOwners[0]

  addIssue(state, {
    code: 'source_identity_conflict',
    severity: 'blocking',
    message: `source ${source.path} is claimed by both ${owner} and ${ideaId}`,
    event_id: eventId,
    idea_id: ideaId,
    event_index: index,
    related_idea_ids: [...new Set([...conflictingOwners, ideaId])],
  })
  markUnsupported(state, owner, eventId, at)
  markUnsupported(state, ideaId, eventId, at, source)
  return false
}

function canTransition(from: IdeaState | undefined, action: NextAction): boolean {
  if (from === undefined) return action !== 'reopen' && action !== 'relink'
  if (action === 'relink') return true
  if (from === 'unsupported') return false
  switch (action) {
    case 'commit':
      return from === 'capture' || from === 'wip' || from === 'waiting'
    case 'wait':
      return from === 'capture' || from === 'wip' || from === 'waiting'
    case 'park':
      return from === 'capture' || from === 'wip' || from === 'waiting'
    case 'settle':
      return from === 'capture' || from === 'wip' || from === 'waiting'
    case 'reopen':
      return from === 'dormant' || from === 'closed'
  }
}

function applyKnownEvent(state: MutableProjection, event: NextEvent, index: number): void {
  const current = state.ideas.get(event.idea_id)
  const source = event.source ?? current?.source
  const project = event.project === null
    ? undefined
    : isNonBlankString(event.project)
      ? event.project
      : current?.project

  if (!current && !event.source) {
    addIssue(state, {
      code: 'missing_initial_source',
      severity: 'blocking',
      message: 'the first disposition for an idea requires source.path',
      event_id: event.event_id,
      idea_id: event.idea_id,
      event_index: index,
      fields: ['source.path'],
    })
    markUnsupported(state, event.idea_id, event.event_id, event.at)
    return
  }

  if (event.source && !claimSource(state, event.source, event.idea_id, event.event_id, event.at, index)) return

  if (current?.source && event.source && event.action !== 'relink' && !sourceMatches(current.source, event.source)) {
    addIssue(state, {
      code: 'source_mismatch',
      severity: 'blocking',
      message: 'a changed source must be recorded with relink',
      event_id: event.event_id,
      idea_id: event.idea_id,
      event_index: index,
      fields: ['source'],
    })
    markUnsupported(state, event.idea_id, event.event_id, event.at, current.source)
    return
  }

  if (!canTransition(current?.state, event.action)) {
    const code = current?.state === 'unsupported' ? 'idea_unsupported' : 'invalid_transition'
    addIssue(state, {
      code,
      severity: current?.state === 'unsupported' ? 'warning' : 'blocking',
      message: current?.state === 'unsupported'
        ? 'the idea has an event this reader cannot safely reduce'
        : `cannot apply ${event.action} from ${current?.state ?? 'no prior state'}`,
      event_id: event.event_id,
      idea_id: event.idea_id,
      event_index: index,
    })
    markUnsupported(state, event.idea_id, event.event_id, event.at, source)
    return
  }

  const base = {
    idea_id: event.idea_id,
    last_event_id: event.event_id,
    last_at: event.at,
    ...(source ? { source } : {}),
    ...(project ? { project } : {}),
  }
  switch (event.action) {
    case 'commit':
      state.ideas.set(event.idea_id, {
        ...base,
        state: 'wip',
        commitment: event.commitment,
        next_action: event.next_action,
        close_condition: event.close_condition,
      })
      break
    case 'wait':
      state.ideas.set(event.idea_id, {
        ...base,
        state: 'waiting',
        waiting_for: event.waiting_for,
        review_at: event.review_at,
      })
      break
    case 'park':
      state.ideas.set(event.idea_id, {
        ...base,
        state: 'dormant',
        wake_trigger: event.wake_trigger,
        ...(event.next_action ? { next_action: event.next_action } : {}),
      })
      break
    case 'settle':
      state.ideas.set(event.idea_id, {
        ...base,
        state: 'closed',
        exit: clonePayload(event.exit),
        ...(event.reason ? { reason: event.reason } : {}),
        ...(event.target ? { target: event.target } : {}),
        ...(event.result ? { result: event.result } : {}),
      })
      break
    case 'reopen':
      state.ideas.set(event.idea_id, { ...base, state: 'capture' })
      break
    case 'relink':
      {
        const { project: _currentProject, ...currentWithoutProject } = current!
        state.ideas.set(event.idea_id, { ...currentWithoutProject, ...base, source: event.source })
      }
      break
  }
}

function applyUnsupportedEvent(state: MutableProjection, event: UnsupportedEvent, index: number): void {
  const current = state.ideas.get(event.idea_id)
  if (!current && !event.source) {
    addIssue(state, {
      code: 'missing_initial_source',
      severity: 'blocking',
      message: 'the first event for an idea requires source.path',
      event_id: event.event_id,
      idea_id: event.idea_id,
      event_index: index,
      fields: ['source.path'],
    })
  }
  if (event.source && !claimSource(state, event.source, event.idea_id, event.event_id, event.at, index)) {
    markUnsupported(state, event.idea_id, event.event_id, event.at, event.source, event.action)
    addIssue(state, {
      code: 'unsupported_action',
      severity: 'warning',
      message: `unsupported action: ${event.action}`,
      event_id: event.event_id,
      idea_id: event.idea_id,
      event_index: index,
    })
    return
  }
  addIssue(state, {
    code: 'unsupported_action',
    severity: 'warning',
    message: `unsupported action: ${event.action}`,
    event_id: event.event_id,
    idea_id: event.idea_id,
    event_index: index,
  })
  markUnsupported(state, event.idea_id, event.event_id, event.at, current?.source ?? event.source, event.action)
}

function finalize(state: MutableProjection): LedgerProjection {
  const values = [...state.ideas.values()]
  const wipCount = values.filter((idea) => idea.state === 'wip').length
  const waitingCount = values.filter((idea) => idea.state === 'waiting').length
  const hasUnsupported = values.some((idea) => idea.state === 'unsupported')
  return {
    ideas: state.ideas,
    eventById: state.eventById,
    sourceToIdeaId: state.sourceToIdeaId,
    issues: state.issues,
    idempotentEventIds: state.idempotentEventIds,
    wipCount,
    waitingCount,
    wipLimit: DEFAULT_WIP_LIMIT,
    waitingWarning: DEFAULT_WAITING_WARNING,
    wipAtLimit: wipCount >= DEFAULT_WIP_LIMIT,
    wipExceeded: wipCount > DEFAULT_WIP_LIMIT,
    waitingExceeded: waitingCount > DEFAULT_WAITING_WARNING,
    hasUnsupported,
    hasBlockingIssues: state.issues.some((issue) => issue.severity === 'blocking'),
  }
}

/**
 * Reduce ledger order exactly as stored. Existing over-limit WIP is always kept
 * visible; the optional hard limit belongs to prospective append validation.
 */
export function reduceEvents(events: readonly unknown[]): LedgerProjection {
  const state: MutableProjection = {
    ideas: new Map(),
    eventById: new Map(),
    sourceToIdeaId: new Map(),
    issues: [],
    idempotentEventIds: [],
  }

  events.forEach((rawEvent, index) => {
    const eventId = envelopeString(rawEvent, 'event_id')
    if (eventId) {
      const previous = state.eventById.get(eventId)
      if (previous !== undefined) {
        if (payloadEqual(previous, rawEvent)) {
          if (!state.idempotentEventIds.includes(eventId)) state.idempotentEventIds.push(eventId)
          return
        }
        const ideaId = envelopeString(rawEvent, 'idea_id')
        const previousIdeaId = envelopeString(previous, 'idea_id')
        addIssue(state, {
          code: 'event_id_conflict',
          severity: 'blocking',
          message: `event_id ${eventId} has different payloads`,
          event_id: eventId,
          ...(ideaId ? { idea_id: ideaId } : {}),
          event_index: index,
          related_idea_ids: [...new Set([previousIdeaId, ideaId].filter(isNonBlankString))],
        })
        const at = envelopeString(rawEvent, 'at') ?? envelopeString(previous, 'at') ?? ''
        if (previousIdeaId) markUnsupported(state, previousIdeaId, eventId, at)
        if (ideaId) {
          const action = envelopeString(rawEvent, 'action')
          markUnsupported(
            state,
            ideaId,
            eventId,
            at,
            envelopeSource(rawEvent),
            action && !isNextAction(action) ? action : undefined,
          )
        }
        return
      }
      state.eventById.set(eventId, clonePayload(rawEvent))
    }

    const validated = validateEvent(rawEvent)
    if (!validated.ok) {
      addIssue(state, issueForInvalidEvent(rawEvent, index, validated.errors))
      const ideaId = envelopeString(rawEvent, 'idea_id')
      const at = envelopeString(rawEvent, 'at') ?? ''
      if (ideaId) {
        const source = envelopeSource(rawEvent)
        if (source && eventId) claimSource(state, source, ideaId, eventId, at, index)
        markUnsupported(state, ideaId, eventId ?? '', at, source, envelopeString(rawEvent, 'action'))
      }
      return
    }

    if (validated.known) applyKnownEvent(state, validated.event, index)
    else applyUnsupportedEvent(state, validated.event, index)
  })

  return finalize(state)
}

function prospectiveIssue(
  code: DomainIssue['code'],
  event: { event_id: string; idea_id: string },
  message: string,
  fields?: readonly string[],
): DomainIssue {
  return {
    code,
    severity: 'blocking',
    message,
    event_id: event.event_id,
    idea_id: event.idea_id,
    ...(fields ? { fields } : {}),
  }
}

/**
 * v1 readers preserve extension fields they did not previously own. The 1.3
 * writer is stricter about the shapes it emits without retroactively making a
 * previously readable ledger invalid.
 */
function writerExtensionErrors(value: unknown): FieldValidationError[] {
  if (!isRecord(value)) return []
  const errors: FieldValidationError[] = []
  if (value.project !== undefined && value.project !== null && !isNonBlankString(value.project)) {
    errors.push({ field: 'project', message: 'must be a non-blank string or null when present' })
  }
  if (value.action !== 'settle' || !isRecord(value.exit) || value.exit.kind !== 'done') return errors
  if (value.exit.delivery === undefined) return errors
  if (value.exit.delivery !== 'article') {
    errors.push({ field: 'exit.delivery', message: 'this writer only supports delivery=article' })
    return errors
  }
  if (value.exit.via !== undefined) {
    errors.push({ field: 'exit', message: 'article delivery cannot also be delegated' })
  }
  if (!isNonBlankString(value.result)) {
    errors.push({ field: 'result', message: 'article delivery requires a result path or URL' })
  }
  return errors
}

/**
 * Validate a human-authored event against a loaded projection before writing.
 * Unknown actions are readable, but this v1 writer never emits them.
 */
export function validateAppend(
  projection: LedgerProjection,
  rawEvent: unknown,
  options: AppendOptions = {},
): AppendValidation {
  const validated = validateEvent(rawEvent)
  if (validated.ok && validated.known) {
    const previous = projection.eventById.get(validated.event.event_id)
    if (previous !== undefined && payloadEqual(previous, rawEvent)) {
      return { ok: true, idempotent: true, event: validated.event }
    }
  }

  const extensionErrors = writerExtensionErrors(rawEvent)
  if (!validated.ok || extensionErrors.length > 0) {
    const errors = [...(!validated.ok ? validated.errors : []), ...extensionErrors]
    return {
      ok: false,
      issues: [{
        code: 'invalid_event',
        severity: 'blocking',
        message: errors.map((error) => `${error.field}: ${error.message}`).join('; '),
        ...(envelopeString(rawEvent, 'event_id') ? { event_id: envelopeString(rawEvent, 'event_id') } : {}),
        ...(envelopeString(rawEvent, 'idea_id') ? { idea_id: envelopeString(rawEvent, 'idea_id') } : {}),
        fields: errors.map((error) => error.field),
      }],
    }
  }
  if (!validated.known) {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'unsupported_action',
        validated.event,
        `this writer does not support action ${validated.event.action}`,
        ['action'],
      )],
    }
  }
  const event = validated.event

  const previous = projection.eventById.get(event.event_id)
  if (previous !== undefined) {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'event_id_conflict',
        event,
        `event_id ${event.event_id} already has a different payload`,
        ['event_id'],
      )],
    }
  }

  // An exact replay is already handled above and changes no state. New writes,
  // however, must stop while any existing event makes the projection unsafe.
  if (projection.hasBlockingIssues) {
    return { ok: false, issues: projection.issues.filter((issue) => issue.severity === 'blocking') }
  }

  const current = projection.ideas.get(event.idea_id)
  if (!current && !event.source) {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'missing_initial_source',
        event,
        'the first disposition for an idea requires source.path',
        ['source.path'],
      )],
    }
  }
  if (event.source) {
    const conflictingOwners = [...ownersOfSource(projection.sourceToIdeaId, event.source)]
      .filter((owner) => owner !== event.idea_id)
    if (conflictingOwners.length > 0) {
      const owner = conflictingOwners[0]
      return {
        ok: false,
        issues: [{
          ...prospectiveIssue(
            'source_identity_conflict',
            event,
            `source ${event.source.path} is already claimed by ${owner}`,
            ['source'],
          ),
          related_idea_ids: [...new Set([...conflictingOwners, event.idea_id])],
        }],
      }
    }
    if (current?.source && event.action !== 'relink' && !sourceMatches(current.source, event.source)) {
      return {
        ok: false,
        issues: [prospectiveIssue(
          'source_mismatch',
          event,
          'a changed source must be recorded with relink',
          ['source'],
        )],
      }
    }
  }
  if (current?.state === 'unsupported' && event.action !== 'relink') {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'idea_unsupported',
        event,
        'cannot transform an idea with an unsupported history',
      )],
    }
  }
  if (!canTransition(current?.state, event.action)) {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'invalid_transition',
        event,
        `cannot apply ${event.action} from ${current?.state ?? 'no prior state'}`,
      )],
    }
  }

  if (options.hardLimit && event.action === 'commit') {
    if (projection.hasUnsupported) {
      return {
        ok: false,
        issues: [prospectiveIssue(
          'wip_limit_uncertain',
          event,
          'cannot enforce the WIP limit while unsupported actions exist',
        )],
      }
    }
    const limit = options.wipLimit ?? projection.wipLimit
    const entersWip = current?.state !== 'wip'
    if (entersWip && projection.wipCount >= limit) {
      return {
        ok: false,
        issues: [prospectiveIssue(
          'wip_limit_exceeded',
          event,
          `WIP limit ${limit} is full`,
        )],
      }
    }
  }

  return { ok: true, idempotent: false, event }
}
