import {
  DEFAULT_WAITING_WARNING,
  DEFAULT_WIP_LIMIT,
  NEXT_ACTIONS,
  itemKey,
  projectTagKey,
  type AppendOptions,
  type AppendValidation,
  type DomainIssue,
  type EventValidation,
  type FieldValidationError,
  type IdeaProjection,
  type IdeaState,
  type ItemKind,
  type ItemProjection,
  type LedgerProjection,
  type NextAction,
  type NextEvent,
  type NormalizedNextEvent,
  type SourceRef,
  type UnsupportedEvent,
} from './model'

type UnknownRecord = Record<string, unknown>

interface MutableProjection {
  items: Map<string, ItemProjection>
  eventById: Map<string, unknown>
  sourceToItemKey: Map<string, string>
  issues: DomainIssue[]
  idempotentEventIds: string[]
}

function isRecord(value: unknown): value is UnknownRecord {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function isNonBlankString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0
}

function readableProjects(value: unknown): string[] | null | undefined {
  if (value === null) return null
  if (!Array.isArray(value) || value.length === 0) return undefined
  const projects = value.map((project) => typeof project === 'string' ? project.trim() : '')
  if (projects.some((project) => !project) || new Set(projects.map(projectTagKey)).size !== projects.length) return undefined
  return projects
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

function itemKind(value: unknown): value is ItemKind {
  return value === 'idea' || value === 'task'
}

function normalizedIdentity(
  record: UnknownRecord,
  errors: FieldValidationError[],
): { item_id: string; item_kind: ItemKind } | null {
  const hasV2Identity = record.item_id !== undefined || record.item_kind !== undefined
  if (!hasV2Identity) {
    if (!isNonBlankString(record.idea_id)) {
      errors.push({ field: 'idea_id', message: 'must be a non-blank string for a v1 event' })
      return null
    }
    return { item_id: record.idea_id, item_kind: 'idea' }
  }

  let valid = true
  if (!isNonBlankString(record.item_id)) {
    errors.push({ field: 'item_id', message: 'must be a non-blank string for a v2 event' })
    valid = false
  }
  if (!itemKind(record.item_kind)) {
    errors.push({ field: 'item_kind', message: 'must be idea or task for a v2 event' })
    valid = false
  }
  if (record.idea_id !== undefined) {
    if (!isNonBlankString(record.idea_id)) {
      errors.push({ field: 'idea_id', message: 'must be a non-blank string when present' })
      valid = false
    } else if (isNonBlankString(record.item_id) && record.idea_id !== record.item_id) {
      errors.push({ field: 'idea_id', message: 'must match item_id when both identity formats are present' })
      valid = false
    }
  }
  return valid ? { item_id: record.item_id as string, item_kind: record.item_kind as ItemKind } : null
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

/** Validate and normalize one v1/v2 event without mutating its persisted record. */
export function validateEvent(value: unknown): EventValidation {
  if (!isRecord(value)) {
    return { ok: false, errors: [{ field: '$', message: 'event must be an object' }] }
  }

  const errors: FieldValidationError[] = []
  requireDate(value, 'at', errors, true)
  requireString(value, 'event_id', errors)
  const identity = normalizedIdentity(value, errors)
  requireString(value, 'action', errors)
  if (value.source !== undefined) validateSource(value.source, 'source', errors)

  if (errors.length > 0) return { ok: false, errors }
  const normalized = { ...clonePayload(value), ...identity } as UnknownRecord
  if (!isNextAction(value.action)) {
    return { ok: true, known: false, event: normalized as UnsupportedEvent }
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
  return { ok: true, known: true, event: normalized as unknown as NormalizedNextEvent }
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

function envelopeIdentity(value: unknown): { item_id: string; item_kind: ItemKind } | undefined {
  if (!isRecord(value)) return undefined
  if (isNonBlankString(value.item_id) && itemKind(value.item_kind)) {
    return { item_id: value.item_id, item_kind: value.item_kind }
  }
  return isNonBlankString(value.idea_id)
    ? { item_id: value.idea_id, item_kind: 'idea' }
    : undefined
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
  itemId: string,
  eventId: string,
  at: string,
  source?: SourceRef,
  action?: string,
  kind?: ItemKind,
): void {
  const resolvedKind = kind ?? 'idea'
  const key = itemKey(resolvedKind, itemId)
  const current = state.items.get(key)
  const actions = unknownActionsOf(current)
  if (action && !actions.includes(action)) actions.push(action)
  const lastKnownState = current?.state === 'unsupported'
    ? current.last_known_state
    : current?.state
  const currentProjects = current?.projects?.length
    ? [...current.projects]
    : current?.project
      ? [current.project]
      : []
  const unsupported: ItemProjection = {
    item_id: itemId,
    item_kind: resolvedKind,
    idea_id: itemId,
    state: 'unsupported',
    last_event_id: eventId,
    last_at: at,
    unsupported_actions: actions,
    ...(currentProjects.length ? { projects: currentProjects, project: currentProjects[0] } : {}),
    ...(current?.source ? { source: current.source } : source ? { source } : {}),
    ...(lastKnownState ? { last_known_state: lastKnownState } : {}),
  }
  state.items.set(key, unsupported)
}

function issueForInvalidEvent(value: unknown, index: number, errors: readonly FieldValidationError[]): DomainIssue {
  const identity = envelopeIdentity(value)
  return {
    code: 'invalid_event',
    severity: 'blocking',
    message: errors.map((error) => `${error.field}: ${error.message}`).join('; '),
    ...(envelopeString(value, 'event_id') ? { event_id: envelopeString(value, 'event_id') } : {}),
    ...(identity ? {
      item_id: identity.item_id,
      item_kind: identity.item_kind,
      ...(identity.item_kind === 'idea' ? { idea_id: identity.item_id } : {}),
    } : {}),
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
  itemId: string,
  eventId: string,
  at: string,
  index: number,
  kind: ItemKind = 'idea',
): boolean {
  const key = sourceKey(source)
  const ownItemKey = itemKey(kind, itemId)
  const owners = ownersOfSource(state.sourceToItemKey, source)
  const conflictingOwners = [...owners].filter((owner) => owner !== ownItemKey)
  if (conflictingOwners.length === 0) {
    state.sourceToItemKey.set(key, ownItemKey)
    return true
  }
  const ownerKey = conflictingOwners[0]
  const owner = state.items.get(ownerKey)

  addIssue(state, {
    code: 'source_identity_conflict',
    severity: 'blocking',
    message: `source ${source.path} is claimed by both ${owner?.item_id ?? ownerKey} and ${itemId}`,
    event_id: eventId,
    item_id: itemId,
    item_kind: kind,
    ...(kind === 'idea' ? { idea_id: itemId } : {}),
    event_index: index,
    related_item_ids: [...new Set([
      ...conflictingOwners.map((key) => state.items.get(key)?.item_id ?? key),
      itemId,
    ])],
    related_idea_ids: [...new Set([
      ...conflictingOwners
        .map((key) => state.items.get(key))
        .filter((item) => item?.item_kind === 'idea')
        .map((item) => item!.item_id),
      ...(kind === 'idea' ? [itemId] : []),
    ])],
  })
  if (owner) markUnsupported(state, owner.item_id, eventId, at, undefined, undefined, owner.item_kind)
  markUnsupported(state, itemId, eventId, at, source, undefined, kind)
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
  const normalized = event as NormalizedNextEvent
  const key = itemKey(normalized.item_kind, normalized.item_id)
  const current = state.items.get(key)
  const source = event.source ?? current?.source
  const eventProjects = readableProjects(event.projects)
  const projects = eventProjects === null
    ? undefined
    : eventProjects
      ? eventProjects
      : event.project === null
        ? undefined
        : isNonBlankString(event.project)
          ? [event.project.trim()]
          : current?.projects
            ? [...current.projects]
            : current?.project
              ? [current.project]
              : undefined

  if (!current && !event.source) {
    addIssue(state, {
      code: 'missing_initial_source',
      severity: 'blocking',
      message: 'the first disposition for an item requires source.path',
      event_id: event.event_id,
      item_id: normalized.item_id,
      item_kind: normalized.item_kind,
      ...(normalized.item_kind === 'idea' ? { idea_id: normalized.item_id } : {}),
      event_index: index,
      fields: ['source.path'],
    })
    markUnsupported(state, normalized.item_id, event.event_id, event.at, undefined, undefined, normalized.item_kind)
    return
  }

  if (event.source && !claimSource(
    state,
    event.source,
    normalized.item_id,
    event.event_id,
    event.at,
    index,
    normalized.item_kind,
  )) return

  if (current?.source && event.source && event.action !== 'relink' && !sourceMatches(current.source, event.source)) {
    addIssue(state, {
      code: 'source_mismatch',
      severity: 'blocking',
      message: 'a changed source must be recorded with relink',
      event_id: event.event_id,
      item_id: normalized.item_id,
      item_kind: normalized.item_kind,
      ...(normalized.item_kind === 'idea' ? { idea_id: normalized.item_id } : {}),
      event_index: index,
      fields: ['source'],
    })
    markUnsupported(state, normalized.item_id, event.event_id, event.at, current.source, undefined, normalized.item_kind)
    return
  }

  if (!canTransition(current?.state, event.action)) {
    const code = current?.state === 'unsupported' ? 'idea_unsupported' : 'invalid_transition'
    addIssue(state, {
      code,
      severity: current?.state === 'unsupported' ? 'warning' : 'blocking',
      message: current?.state === 'unsupported'
        ? 'the item has an event this reader cannot safely reduce'
        : `cannot apply ${event.action} from ${current?.state ?? 'no prior state'}`,
      event_id: event.event_id,
      item_id: normalized.item_id,
      item_kind: normalized.item_kind,
      ...(normalized.item_kind === 'idea' ? { idea_id: normalized.item_id } : {}),
      event_index: index,
    })
    markUnsupported(state, normalized.item_id, event.event_id, event.at, source, undefined, normalized.item_kind)
    return
  }

  const base = {
    item_id: normalized.item_id,
    item_kind: normalized.item_kind,
    idea_id: normalized.item_id,
    last_event_id: event.event_id,
    last_at: event.at,
    ...(source ? { source } : {}),
    ...(projects?.length ? { projects, project: projects[0] } : {}),
  }
  switch (event.action) {
    case 'commit':
      state.items.set(key, {
        ...base,
        state: 'wip',
        commitment: event.commitment,
        next_action: event.next_action,
        close_condition: event.close_condition,
      })
      break
    case 'wait':
      state.items.set(key, {
        ...base,
        state: 'waiting',
        waiting_for: event.waiting_for,
        review_at: event.review_at,
      })
      break
    case 'park':
      state.items.set(key, {
        ...base,
        state: 'dormant',
        wake_trigger: event.wake_trigger,
        ...(event.next_action ? { next_action: event.next_action } : {}),
      })
      break
    case 'settle':
      state.items.set(key, {
        ...base,
        state: 'closed',
        exit: clonePayload(event.exit),
        ...(event.reason ? { reason: event.reason } : {}),
        ...(event.target ? { target: event.target } : {}),
        ...(event.result ? { result: event.result } : {}),
      })
      break
    case 'reopen':
      state.items.set(key, { ...base, state: 'capture' })
      break
    case 'relink':
      {
        const { project: _currentProject, projects: _currentProjects, ...currentWithoutProject } = current!
        state.items.set(key, { ...currentWithoutProject, ...base, source: event.source })
      }
      break
  }
}

function applyUnsupportedEvent(state: MutableProjection, event: UnsupportedEvent, index: number): void {
  const current = state.items.get(itemKey(event.item_kind, event.item_id))
  if (!current && !event.source) {
    addIssue(state, {
      code: 'missing_initial_source',
      severity: 'blocking',
      message: 'the first event for an item requires source.path',
      event_id: event.event_id,
      item_id: event.item_id,
      item_kind: event.item_kind,
      ...(event.item_kind === 'idea' ? { idea_id: event.item_id } : {}),
      event_index: index,
      fields: ['source.path'],
    })
  }
  if (event.source && !claimSource(
    state,
    event.source,
    event.item_id,
    event.event_id,
    event.at,
    index,
    event.item_kind,
  )) {
    markUnsupported(state, event.item_id, event.event_id, event.at, event.source, event.action, event.item_kind)
    addIssue(state, {
      code: 'unsupported_action',
      severity: 'warning',
      message: `unsupported action: ${event.action}`,
      event_id: event.event_id,
      item_id: event.item_id,
      item_kind: event.item_kind,
      ...(event.item_kind === 'idea' ? { idea_id: event.item_id } : {}),
      event_index: index,
    })
    return
  }
  addIssue(state, {
    code: 'unsupported_action',
    severity: 'warning',
    message: `unsupported action: ${event.action}`,
    event_id: event.event_id,
    item_id: event.item_id,
    item_kind: event.item_kind,
    ...(event.item_kind === 'idea' ? { idea_id: event.item_id } : {}),
    event_index: index,
  })
  markUnsupported(state, event.item_id, event.event_id, event.at, current?.source ?? event.source, event.action, event.item_kind)
}

function effectiveWipLimit(value: number | undefined): number {
  if (value === undefined || !Number.isSafeInteger(value) || value < 1) return DEFAULT_WIP_LIMIT
  return value
}

function finalize(state: MutableProjection, configuredWipLimit?: number): LedgerProjection {
  const values = [...state.items.values()]
  const ideas = new Map(values
    .filter((item) => item.item_kind === 'idea')
    .map((item) => [item.item_id, item] as const))
  const sourceToIdeaId = new Map<string, string>()
  for (const [source, key] of state.sourceToItemKey) {
    const item = state.items.get(key)
    if (item?.item_kind === 'idea') sourceToIdeaId.set(source, item.item_id)
  }
  const wipCount = values.filter((idea) => idea.state === 'wip').length
  const waitingCount = values.filter((idea) => idea.state === 'waiting').length
  const hasUnsupported = values.some((idea) => idea.state === 'unsupported')
  const wipLimit = effectiveWipLimit(configuredWipLimit)
  return {
    items: state.items,
    ideas,
    eventById: state.eventById,
    sourceToIdeaId,
    sourceToItemKey: state.sourceToItemKey,
    issues: state.issues,
    idempotentEventIds: state.idempotentEventIds,
    wipCount,
    waitingCount,
    wipLimit,
    waitingWarning: DEFAULT_WAITING_WARNING,
    wipAtLimit: wipCount >= wipLimit,
    wipExceeded: wipCount > wipLimit,
    waitingExceeded: waitingCount > DEFAULT_WAITING_WARNING,
    hasUnsupported,
    hasBlockingIssues: state.issues.some((issue) => issue.severity === 'blocking'),
  }
}

/**
 * Reduce ledger order exactly as stored. Existing over-limit WIP is always kept
 * visible; the optional hard limit belongs to prospective append validation.
 */
export function reduceEvents(
  events: readonly unknown[],
  options: { wipLimit?: number } = {},
): LedgerProjection {
  const state: MutableProjection = {
    items: new Map(),
    eventById: new Map(),
    sourceToItemKey: new Map(),
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
        const identity = envelopeIdentity(rawEvent)
        const previousIdentity = envelopeIdentity(previous)
        const itemId = identity?.item_id
        const previousItemId = previousIdentity?.item_id
        addIssue(state, {
          code: 'event_id_conflict',
          severity: 'blocking',
          message: `event_id ${eventId} has different payloads`,
          event_id: eventId,
          ...(identity ? {
            item_id: identity.item_id,
            item_kind: identity.item_kind,
            ...(identity.item_kind === 'idea' ? { idea_id: identity.item_id } : {}),
          } : {}),
          event_index: index,
          related_item_ids: [...new Set([previousItemId, itemId].filter(isNonBlankString))],
          related_idea_ids: [...new Set([
            previousIdentity?.item_kind === 'idea' ? previousItemId : undefined,
            identity?.item_kind === 'idea' ? itemId : undefined,
          ].filter(isNonBlankString))],
        })
        const at = envelopeString(rawEvent, 'at') ?? envelopeString(previous, 'at') ?? ''
        if (previousItemId) markUnsupported(
          state,
          previousItemId,
          eventId,
          at,
          undefined,
          undefined,
          previousIdentity?.item_kind,
        )
        if (itemId) {
          const action = envelopeString(rawEvent, 'action')
          markUnsupported(
            state,
            itemId,
            eventId,
            at,
            envelopeSource(rawEvent),
            action && !isNextAction(action) ? action : undefined,
            identity?.item_kind,
          )
        }
        return
      }
      state.eventById.set(eventId, clonePayload(rawEvent))
    }

    const validated = validateEvent(rawEvent)
    if (!validated.ok) {
      addIssue(state, issueForInvalidEvent(rawEvent, index, validated.errors))
      const identity = envelopeIdentity(rawEvent)
      const itemId = identity?.item_id
      const at = envelopeString(rawEvent, 'at') ?? ''
      if (itemId) {
        const source = envelopeSource(rawEvent)
        if (source && eventId) claimSource(state, source, itemId, eventId, at, index, identity?.item_kind)
        markUnsupported(
          state,
          itemId,
          eventId ?? '',
          at,
          source,
          envelopeString(rawEvent, 'action'),
          identity?.item_kind,
        )
      }
      return
    }

    if (validated.known) applyKnownEvent(state, validated.event, index)
    else applyUnsupportedEvent(state, validated.event, index)
  })

  return finalize(state, options.wipLimit)
}

function prospectiveIssue(
  code: DomainIssue['code'],
  event: { event_id: string; item_id: string; item_kind: ItemKind },
  message: string,
  fields?: readonly string[],
): DomainIssue {
  return {
    code,
    severity: 'blocking',
    message,
    event_id: event.event_id,
    item_id: event.item_id,
    item_kind: event.item_kind,
    ...(event.item_kind === 'idea' ? { idea_id: event.item_id } : {}),
    ...(fields ? { fields } : {}),
  }
}

/**
 * v1 readers preserve extension fields they did not previously own. The 1.3
 * writer is stricter about the shapes it emits without retroactively making a
 * previously readable ledger invalid.
 */
function writerExtensionErrors(value: unknown, ledgerVersion?: 1 | 2): FieldValidationError[] {
  if (!isRecord(value)) return []
  const errors: FieldValidationError[] = []
  const hasV2Identity = value.item_id !== undefined || value.item_kind !== undefined
  if (ledgerVersion === 2 && !hasV2Identity) {
    errors.push({ field: 'item_id', message: 'v2 ledger events must use item_id and item_kind' })
  }
  if (ledgerVersion === 2 && value.idea_id !== undefined) {
    errors.push({ field: 'idea_id', message: 'v2 ledger events must not emit the legacy idea_id field' })
  }
  if (ledgerVersion === 1 && hasV2Identity) {
    errors.push({ field: 'item_id', message: 'v2 item identities require a v2 ledger' })
  }
  if (value.project !== undefined && value.project !== null && !isNonBlankString(value.project)) {
    errors.push({ field: 'project', message: 'must be a non-blank string or null when present' })
  }
  if (value.projects !== undefined) {
    if (value.projects === null) {
      if (value.project !== null) errors.push({ field: 'project', message: 'must be null when projects is null' })
    } else if (!Array.isArray(value.projects) || value.projects.length === 0) {
      errors.push({ field: 'projects', message: 'must be a non-empty string array or null when present' })
    } else {
      const projects = value.projects
      const normalized = projects.map((project) => typeof project === 'string' ? project.trim() : '')
      if (normalized.some((project, index) => !project || project !== projects[index])) {
        errors.push({ field: 'projects', message: 'must contain trimmed non-blank strings' })
      } else if (new Set(normalized.map(projectTagKey)).size !== normalized.length) {
        errors.push({ field: 'projects', message: 'must not contain duplicates' })
      }
      if (value.project !== normalized[0]) {
        errors.push({ field: 'project', message: 'must mirror the first projects entry' })
      }
    }
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

  const extensionErrors = writerExtensionErrors(rawEvent, options.ledgerVersion)
  if (!validated.ok || extensionErrors.length > 0) {
    const errors = [...(!validated.ok ? validated.errors : []), ...extensionErrors]
    return {
      ok: false,
      issues: [{
        code: 'invalid_event',
        severity: 'blocking',
        message: errors.map((error) => `${error.field}: ${error.message}`).join('; '),
        ...(envelopeString(rawEvent, 'event_id') ? { event_id: envelopeString(rawEvent, 'event_id') } : {}),
        ...(envelopeIdentity(rawEvent) ? {
          item_id: envelopeIdentity(rawEvent)!.item_id,
          item_kind: envelopeIdentity(rawEvent)!.item_kind,
          ...(envelopeIdentity(rawEvent)!.item_kind === 'idea'
            ? { idea_id: envelopeIdentity(rawEvent)!.item_id }
            : {}),
        } : {}),
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

  const current = projection.items.get(itemKey(event.item_kind, event.item_id))
  if (!current && !event.source) {
    return {
      ok: false,
      issues: [prospectiveIssue(
        'missing_initial_source',
        event,
        'the first disposition for an item requires source.path',
        ['source.path'],
      )],
    }
  }
  if (event.source) {
    const ownItemKey = itemKey(event.item_kind, event.item_id)
    const conflictingOwners = [...ownersOfSource(projection.sourceToItemKey, event.source)]
      .filter((owner) => owner !== ownItemKey)
    if (conflictingOwners.length > 0) {
      const owner = projection.items.get(conflictingOwners[0])
      return {
        ok: false,
        issues: [{
          ...prospectiveIssue(
            'source_identity_conflict',
            event,
            `source ${event.source.path} is already claimed by ${owner?.item_id ?? conflictingOwners[0]}`,
            ['source'],
          ),
          related_item_ids: [...new Set([
            ...conflictingOwners.map((key) => projection.items.get(key)?.item_id ?? key),
            event.item_id,
          ])],
          related_idea_ids: event.item_kind === 'idea'
            ? [...new Set([
              ...conflictingOwners
                .map((key) => projection.items.get(key))
                .filter((item) => item?.item_kind === 'idea')
                .map((item) => item!.item_id),
              event.item_id,
            ])]
            : conflictingOwners
              .map((key) => projection.items.get(key))
              .filter((item) => item?.item_kind === 'idea')
              .map((item) => item!.item_id),
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
        'cannot transform an item with an unsupported history',
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
