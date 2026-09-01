import type {
  CommitEvent,
  ItemKind,
  NextEvent,
  ParkEvent,
  RelinkEvent,
  SettleEvent,
  SettlementExit,
  WaitEvent,
} from './model'
import { uniqueProjectTags } from './model'
import { sourceRefOf, type WorkspaceItem } from './repository'
import type { IdeaSource } from './source'
import type { LedgerVersion } from './ledger'

type LifecycleItem = WorkspaceItem & {
  item_id?: string
  item_kind?: ItemKind
  kind?: ItemKind
}

type ProjectChange = {
  projects?: string[] | null
  /** Deprecated input accepted for callers before 1.4. */
  project?: string | null
}

export type PlaceInput = (
  | { route: 'commit'; commitment: string; next_action: string; close_condition: string }
  | { route: 'wait'; waiting_for: string; review_at: string }
  | { route: 'park'; wake_trigger: string; next_action?: string }
  | {
      route: 'settle'
      exit: SettlementExit
      reason?: string
      target?: string
      result?: string
    }
) & ProjectChange

export interface EventFactory {
  now(): string
  id(): string
}

export const defaultEventFactory: EventFactory = {
  now: () => new Date().toISOString(),
  id: () => {
    if (typeof globalThis.crypto?.randomUUID === 'function') return globalThis.crypto.randomUUID()
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`
  },
}

function clean(value: string | undefined): string | undefined {
  const result = value?.trim()
  return result ? result : undefined
}

function normalizedProjects(values: readonly string[]): string[] {
  return uniqueProjectTags(values)
}

function projectChange(input: ProjectChange): { projects?: string[] | null; project?: string | null } {
  if (input.projects === null || (input.projects === undefined && input.project === null)) {
    return { projects: null, project: null }
  }
  const projects = input.projects === undefined
    ? typeof input.project === 'string' ? normalizedProjects([input.project]) : []
    : normalizedProjects(input.projects)
  return projects.length ? { projects, project: projects[0] } : {}
}

function envelope(item: LifecycleItem, factory: EventFactory, version: LedgerVersion) {
  const source = sourceRefOf(item)
  const kind = item.item_kind ?? item.kind ?? item.projection?.item_kind ?? 'idea'
  const existingId = item.item_id ?? item.projection?.item_id ?? item.idea_id
  if (kind === 'task' && !existingId) throw new Error('a task requires its stable item_id')
  if (!existingId && !source) throw new Error('a new item requires a source')
  const at = factory.now()
  const eventId = factory.id()
  const id = existingId ?? factory.id()
  return {
    at,
    event_id: eventId,
    ...(version === 2 || kind === 'task'
      ? { item_id: id, item_kind: kind }
      : { idea_id: id }),
    ...(source ? { source } : {}),
  }
}

export function placeEvent(
  item: LifecycleItem,
  input: PlaceInput,
  factory: EventFactory = defaultEventFactory,
  version: LedgerVersion = 1,
): NextEvent {
  const base = { ...envelope(item, factory, version), ...projectChange(input) }
  switch (input.route) {
    case 'commit': {
      const event: CommitEvent = {
        ...base,
        action: 'commit',
        commitment: input.commitment.trim(),
        next_action: input.next_action.trim(),
        close_condition: input.close_condition.trim(),
      }
      return event
    }
    case 'wait': {
      const event: WaitEvent = {
        ...base,
        action: 'wait',
        waiting_for: input.waiting_for.trim(),
        review_at: input.review_at.trim(),
      }
      return event
    }
    case 'park': {
      const nextAction = clean(input.next_action)
      const event: ParkEvent = {
        ...base,
        action: 'park',
        wake_trigger: input.wake_trigger.trim(),
        ...(nextAction ? { next_action: nextAction } : {}),
      }
      return event
    }
    case 'settle': {
      const reason = clean(input.reason)
      const target = clean(input.target)
        ?? (input.exit.kind === 'transferred'
          && input.exit.via === 'project'
          && base.projects?.length === 1
          ? base.projects[0]
          : undefined)
      const result = clean(input.result)
      const event: SettleEvent = {
        ...base,
        action: 'settle',
        exit: input.exit,
        ...(reason ? { reason } : {}),
        ...(target ? { target } : {}),
        ...(result ? { result } : {}),
      }
      return event
    }
  }
}

export function reopenEvent(
  item: LifecycleItem,
  factory: EventFactory = defaultEventFactory,
  version: LedgerVersion = 1,
): NextEvent {
  return { ...envelope(item, factory, version), action: 'reopen' }
}

export function relinkEvent(
  item: LifecycleItem,
  source: IdeaSource,
  factory: EventFactory = defaultEventFactory,
  version: LedgerVersion = 1,
): RelinkEvent {
  const kind = item.item_kind ?? item.kind ?? item.projection?.item_kind ?? 'idea'
  const id = item.item_id ?? item.projection?.item_id ?? item.idea_id
  if (!id) throw new Error('only a placed item can be relinked')
  return {
    at: factory.now(),
    event_id: factory.id(),
    ...(version === 2 || kind === 'task'
      ? { item_id: id, item_kind: kind }
      : { idea_id: id }),
    action: 'relink',
    source: { path: source.path, ...(source.created ? { created: source.created } : {}) },
  }
}
