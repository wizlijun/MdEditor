import { placeEvent, relinkEvent, reopenEvent, type PlaceInput } from './events'
import {
  appendEvent,
  createIdeaSource,
  loadWorkspace,
  openSource,
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
  await save(placeEvent(item, input))
}

export async function reopen(item: WorkspaceItem): Promise<void> {
  await save(reopenEvent(item))
}

export async function relink(item: WorkspaceItem, source: IdeaSource): Promise<void> {
  await save(relinkEvent(item, source))
}

export async function open(item: WorkspaceItem): Promise<void> {
  await openSource(item)
}
