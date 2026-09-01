import { placeEvent, relinkEvent, reopenEvent, type PlaceInput } from './events'
import {
  appendEvent,
  createIdeaSource,
  createTaskSource,
  loadWorkspace,
  openSource,
  type CreateTaskInput,
  type NextWorkspace,
  type WorkspaceItem,
} from './repository'
import type { IdeaSource } from './source'

export const state = $state<{
  workspace: NextWorkspace | null
  loading: boolean
  saving: boolean
  error: string | null
}>({
  workspace: null,
  loading: true,
  saving: false,
  error: null,
})

export async function refresh(): Promise<void> {
  if (state.saving) return
  // Loading may persist source_dirs. Share the same critical section as event
  // appends so a focus refresh cannot overwrite a just-saved decision.
  state.saving = true
  state.loading = state.workspace === null
  state.error = null
  try {
    state.workspace = await loadWorkspace()
  } catch (error) {
    state.error = String(error)
    throw error
  } finally {
    state.loading = false
    state.saving = false
  }
}

export async function createIdea(body: string): Promise<string> {
  if (state.saving) throw new Error('Next is already saving')
  state.saving = true
  state.error = null
  try {
    const created = await createIdeaSource(body)
    state.workspace = await loadWorkspace()
    return created.path
  } catch (error) {
    state.error = String(error)
    throw error
  } finally {
    state.saving = false
  }
}

export interface CreateTaskResult {
  path: string
  placedCurrent: boolean
  refreshError?: string
  placementError?: string
}

export async function createTask(input: CreateTaskInput, markCurrent: boolean): Promise<CreateTaskResult> {
  if (state.saving) throw new Error('Next is already saving')
  state.saving = true
  state.error = null
  try {
    // Source-first is deliberate: a lifecycle write can safely degrade to an
    // Inbox Task, while reversing the order could create an orphaned event.
    const created = await createTaskSource(input)
    let inbox: NextWorkspace
    try {
      inbox = await loadWorkspace()
    } catch (error) {
      // Publication is already the durable commit point. Report a refresh
      // warning instead of telling the user creation failed and inviting a
      // duplicate retry.
      return {
        path: created.path,
        placedCurrent: false,
        refreshError: String(error),
      }
    }
    state.workspace = inbox
    if (!markCurrent) return { path: created.path, placedCurrent: false }

    try {
      const item = inbox.capture.find((candidate) => (
        candidate.kind === 'task'
          && candidate.item_id === created.source.task.id
          && candidate.path === created.path
          && !candidate.repairReason
          && !candidate.orphan
      ))
      if (!item) throw new Error('Created Task is not available for placement')
      const event = placeEvent(item, {
        route: 'commit',
        commitment: input.title.trim(),
        next_action: input.title.trim(),
        close_condition: input.done_when?.trim() ?? '',
      }, undefined, inbox.ledger.version)
      // The shortcut must not silently exceed the real shared WIP capacity;
      // failure safely degrades to the already-created Inbox Task.
      state.workspace = await appendEvent(inbox, event, { hardWipLimit: true })
      return { path: created.path, placedCurrent: true }
    } catch (error) {
      // The Task file is already durable. Rebuild from disk so the UI shows it
      // in Inbox and return a result distinct from source creation failure.
      try {
        state.workspace = await loadWorkspace()
      } catch {
        // Keep the successfully loaded Inbox snapshot and the placement error.
      }
      return {
        path: created.path,
        placedCurrent: false,
        placementError: String(error),
      }
    }
  } catch (error) {
    state.error = String(error)
    throw error
  } finally {
    state.saving = false
  }
}

async function save(event: ReturnType<typeof placeEvent>): Promise<void> {
  if (!state.workspace) throw new Error('Next is not loaded')
  if (state.saving) throw new Error('Next is already saving')
  state.saving = true
  state.error = null
  try {
    // v1 is the preregistered soft-limit phase. The domain supports the G3
    // hard-limit experiment, but normal product writes deliberately do not.
    state.workspace = await appendEvent(state.workspace, event, { hardWipLimit: false })
  } catch (error) {
    state.error = String(error)
    try {
      state.workspace = await loadWorkspace()
    } catch {
      // Preserve the original write error; refresh failure will surface on the
      // next explicit/focus refresh without hiding why this action failed.
    }
    throw error
  } finally {
    state.saving = false
  }
}

export async function place(item: WorkspaceItem, input: PlaceInput): Promise<void> {
  if (!state.workspace) throw new Error('Next is not loaded')
  await save(placeEvent(item, input, undefined, state.workspace.ledger.version))
}

export async function reopen(item: WorkspaceItem): Promise<void> {
  if (!state.workspace) throw new Error('Next is not loaded')
  await save(reopenEvent(item, undefined, state.workspace.ledger.version))
}

export async function relink(item: WorkspaceItem, source: IdeaSource): Promise<void> {
  if (!state.workspace) throw new Error('Next is not loaded')
  await save(relinkEvent(item, source, undefined, state.workspace.ledger.version))
}

export async function open(item: WorkspaceItem): Promise<void> {
  await openSource(item)
}
