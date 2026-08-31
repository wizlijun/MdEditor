import type {
  CommitEvent,
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

function envelope(item: WorkspaceItem, factory: EventFactory) {
  const source = sourceRefOf(item)
  if (!item.idea_id && !source) throw new Error('a new idea requires a source')
  return {
    at: factory.now(),
    event_id: factory.id(),
    idea_id: item.idea_id ?? factory.id(),
    ...(source ? { source } : {}),
  }
}

export function placeEvent(
  item: WorkspaceItem,
  input: PlaceInput,
  factory: EventFactory = defaultEventFactory,
): NextEvent {
  const base = { ...envelope(item, factory), ...projectChange(input) }
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
  item: WorkspaceItem,
  factory: EventFactory = defaultEventFactory,
): NextEvent {
  return { ...envelope(item, factory), action: 'reopen' }
}

export function relinkEvent(
  item: WorkspaceItem,
  source: IdeaSource,
  factory: EventFactory = defaultEventFactory,
): RelinkEvent {
  if (!item.idea_id) throw new Error('only a placed idea can be relinked')
  return {
    at: factory.now(),
    event_id: factory.id(),
    idea_id: item.idea_id,
    action: 'relink',
    source: { path: source.path, ...(source.created ? { created: source.created } : {}) },
  }
}
