import {
  editorOpen,
  vaultExists,
  vaultList,
  vaultRead,
  vaultRemove,
  vaultRename,
  vaultWrite,
  type VaultEntry,
} from './bridge'
import { ownersOfSource, reduceEvents, validateAppend } from './domain'
import { NEXT_PATH, newLedger, parseLedger, serializeLedger, upgradeLedgerToV2, type LedgerDocument } from './ledger'
import { itemKey, projectTagKey, projectTagsOf, type IdeaProjection, type ItemKind, type LedgerProjection, type NextEvent, type SourceRef } from './model'
import { buildProjectMatcher, type ProjectSuggestion } from './project-match'
import {
  buildIdeaDocument,
  DEFAULT_IDEA_DIR,
  IDEA_SPARK_STATE_PATH,
  isIdeaFileName,
  parseIdeaDir,
  parseIdeaSource,
  proofPathFor,
  sortIdeasNewestFirst,
  timestampIdeaFileName,
  type IdeaSource,
} from './source'
import {
  buildTaskDocument,
  DEFAULT_TASK_DIR,
  isTaskFileName,
  parseTaskSource,
  TASK_SUFFIX,
  taskIdentityHint,
  timestampTaskFileName,
  type TaskDetails,
  type TaskSource,
} from './task-source'

export interface VaultPort {
  exists(path: string): Promise<{ exists: boolean }>
  list(path: string): Promise<{ entries: VaultEntry[] }>
  read(path: string): Promise<{ content: string }>
  write(path: string, content: string): Promise<{ ok: true }>
  rename(from: string, to: string): Promise<{ ok: true }>
  remove(path: string): Promise<{ ok: true }>
  open(path: string): Promise<unknown>
}

export const hostVault: VaultPort = {
  exists: vaultExists,
  list: vaultList,
  read: vaultRead,
  write: vaultWrite,
  rename: (from, to) => vaultRename(from, to),
  remove: (path) => vaultRemove(path),
  open: editorOpen,
}

export interface WorkspaceItem {
  key: string
  /** Omitted by legacy fixtures and treated as Idea. Loaded items always set it. */
  kind?: ItemKind
  item_id?: string
  item_kind?: ItemKind
  idea_id?: string
  state: IdeaProjection['state'] | 'capture'
  title: string
  description?: string
  body?: string
  path?: string
  created?: string
  proofed: boolean
  orphan: boolean
  projection?: IdeaProjection
  relinkCandidates: IdeaSource[]
  relinkMatch: 'created' | 'manual' | null
  task?: TaskDetails
  generatedBy?: string
  /** Source-level validation issue shown in the repair area. */
  repairReason?: string
  /** Local, non-persistent suggestion. It is never a confirmed project tag. */
  suggestedProject?: ProjectSuggestion
}

export interface NextWorkspace {
  ledger: LedgerDocument
  ledgerRaw: string | null
  sourceDirs: string[]
  /** Idea Spark's current capture directory; historic sourceDirs remain separate. */
  ideaDir: string
  taskDir?: string
  projection: LedgerProjection
  sources: IdeaSource[]
  taskSources?: TaskSource[]
  items: WorkspaceItem[]
  capture: WorkspaceItem[]
  wip: WorkspaceItem[]
  waiting: WorkspaceItem[]
  dormant: WorkspaceItem[]
  closed: WorkspaceItem[]
  unsupported: WorkspaceItem[]
  /** Current project markers plus legacy project-transfer targets, newest first. */
  projectOptions: string[]
  scanErrors: string[]
  readOnlyError: string | null
}

export class NextWriteError extends Error {
  constructor(
    readonly code: 'read_only' | 'changed_missing' | 'invalid_event' | 'write_verification_failed',
    message: string,
  ) {
    super(message)
    this.name = 'NextWriteError'
  }
}

function unique(values: string[]): string[] {
  return [...new Set(values)]
}

async function currentIdeaDir(port: VaultPort): Promise<string> {
  try {
    if (!(await port.exists(IDEA_SPARK_STATE_PATH)).exists) return DEFAULT_IDEA_DIR
    return parseIdeaDir((await port.read(IDEA_SPARK_STATE_PATH)).content)
  } catch {
    return DEFAULT_IDEA_DIR
  }
}

export interface CreatedIdea {
  path: string
  content: string
}

export interface CreateTaskInput {
  title: string
  body?: string
  done_when?: string
}

export interface CreatedTask {
  path: string
  content: string
  source: TaskSource
}

function newTaskId(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') return globalThis.crypto.randomUUID()
  if (typeof globalThis.crypto?.getRandomValues !== 'function') {
    throw new Error('Secure UUID generation is unavailable')
  }
  const bytes = new Uint8Array(16)
  globalThis.crypto.getRandomValues(bytes)
  bytes[6] = (bytes[6]! & 0x0f) | 0x40
  bytes[8] = (bytes[8]! & 0x3f) | 0x80
  const hex = [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

/**
 * Creates one new source Idea without producing a Next lifecycle event.
 * Existing ideas and proof sidecars are only checked for collisions, never changed.
 */
export async function createIdeaSource(
  body: string,
  options: { now?: () => Date } = {},
  port: VaultPort = hostVault,
): Promise<CreatedIdea> {
  if (!body.trim()) throw new Error('Idea body cannot be blank')
  const ideaDir = await currentIdeaDir(port)
  const now = options.now?.() ?? new Date()
  const taken = new Set<string>()

  for (let attempt = 0; attempt < 100; attempt += 1) {
    const name = timestampIdeaFileName(now, taken)
    const path = `${ideaDir}/${name}`
    const proofPath = `${ideaDir}/${proofPathFor(name)}`
    const [ideaOccupied, proofOccupied] = await Promise.all([
      port.exists(path),
      port.exists(proofPath),
    ])
    if (ideaOccupied.exists || proofOccupied.exists) {
      taken.add(name)
      continue
    }

    const content = buildIdeaDocument(body, now.toISOString())
    await port.write(path, content)
    const written = (await port.read(path)).content
    if (written !== content) throw new Error('Created Idea did not match after writing')
    return { path, content }
  }

  throw new Error('Could not find a free Idea filename after 100 attempts')
}

/**
 * Creates one Task through a verified same-directory temporary file. The host
 * rename is the create-only publication boundary; an existing final path is
 * never overwritten.
 */
export async function createTaskSource(
  input: CreateTaskInput,
  options: { now?: () => Date; id?: () => string } = {},
  port: VaultPort = hostVault,
): Promise<CreatedTask> {
  const title = input.title.trim()
  if (!title) throw new Error('Task title cannot be blank')
  const now = options.now?.() ?? new Date()
  const id = options.id?.() ?? newTaskId()
  const content = buildTaskDocument({
    title,
    created: now.toISOString(),
    task: {
      version: 1,
      id,
      ...(input.done_when?.trim() ? { done_when: input.done_when.trim() } : {}),
    },
    ...(input.body?.trim() ? { body: input.body.trim() } : {}),
  })
  const tempPath = `${DEFAULT_TASK_DIR}/.next-task-${id}.tmp`
  if ((await port.exists(tempPath)).exists) throw new Error('Task temporary file already exists')

  let published = false
  try {
    await port.write(tempPath, content)
    const written = (await port.read(tempPath)).content
    if (written !== content) throw new Error('Created Task did not match after writing')
    // Parsing the exact bytes verifies the complete Task schema. The parser's
    // path contract intentionally sees a representative final path.
    parseTaskSource(`${DEFAULT_TASK_DIR}/verified-task.md`, written)

    const taken = new Set<string>()
    if ((await port.exists(DEFAULT_TASK_DIR)).exists) {
      for (const entry of (await port.list(DEFAULT_TASK_DIR)).entries) taken.add(entry.name)
    }
    for (let attempt = 0; attempt < 100; attempt += 1) {
      const name = timestampTaskFileName(now, title, taken)
      const path = `${DEFAULT_TASK_DIR}/${name}`
      try {
        await port.rename(tempPath, path)
        published = true
        // The temporary bytes were already read back and parsed. Host rename
        // is an atomic no-clobber move and does not transform those bytes, so
        // publication success is the durable commit point. A second read here
        // would only create an ambiguous failure after the file already exists.
        return { path, content, source: parseTaskSource(path, content) }
      } catch (error) {
        if ((await port.exists(path)).exists && (await port.exists(tempPath)).exists) {
          taken.add(name)
          continue
        }
        throw error
      }
    }
    throw new Error('Could not find a free Task filename after 100 attempts')
  } finally {
    if (!published) {
      try {
        await port.remove(tempPath)
      } catch {
        // Preserve the creation error. Cleanup is best effort and remove is
        // intentionally scoped to the one known temporary file.
      }
    }
  }
}

interface ScanResult {
  sources: IdeaSource[]
  errors: string[]
}

interface TaskScanResult {
  sources: TaskSource[]
  repairs: WorkspaceItem[]
  errors: string[]
}

export async function scanIdeaDirs(sourceDirs: string[], port: VaultPort = hostVault): Promise<ScanResult> {
  const sources: IdeaSource[] = []
  const errors: string[] = []
  for (const dir of sourceDirs) {
    let entries: VaultEntry[]
    try {
      if (!(await port.exists(dir)).exists) continue
      entries = (await port.list(dir)).entries
    } catch (error) {
      errors.push(`${dir}: ${String(error)}`)
      continue
    }
    const fileNames = new Set(entries.filter((entry) => !entry.is_dir).map((entry) => entry.name))
    const ideaNames = [...fileNames].filter(isIdeaFileName)
    for (const name of ideaNames) {
      const path = `${dir}/${name}`
      try {
        const content = (await port.read(path)).content
        const proofName = proofPathFor(name)
        sources.push(parseIdeaSource(path, content, fileNames.has(proofName)))
      } catch (error) {
        errors.push(`${path}: ${String(error)}`)
      }
    }
  }
  return { sources: sortIdeasNewestFirst(sources), errors }
}

export async function scanTaskDir(port: VaultPort = hostVault): Promise<TaskScanResult> {
  const parsed: TaskSource[] = []
  const repairs: WorkspaceItem[] = []
  const errors: string[] = []
  try {
    if (!(await port.exists(DEFAULT_TASK_DIR)).exists) return { sources: [], repairs: [], errors: [] }
    const entries = (await port.list(DEFAULT_TASK_DIR)).entries
    for (const name of entries.filter((entry) => !entry.is_dir).map((entry) => entry.name).filter(isTaskFileName)) {
      const path = `${DEFAULT_TASK_DIR}/${name}`
      let content = ''
      try {
        content = (await port.read(path)).content
        parsed.push(parseTaskSource(path, content))
      } catch (error) {
        const repairReason = String(error)
        const hint = taskIdentityHint(content)
        errors.push(`${path}: ${repairReason}`)
        repairs.push({
          key: `task-repair:${path}`,
          kind: 'task',
          ...(hint.id ? { item_id: hint.id } : {}),
          item_kind: 'task',
          state: 'unsupported',
          title: hint.title ?? name.slice(0, -TASK_SUFFIX.length),
          path,
          proofed: false,
          orphan: false,
          relinkCandidates: [],
          relinkMatch: null,
          repairReason,
        })
      }
    }
  } catch (error) {
    return { sources: [], repairs, errors: [`${DEFAULT_TASK_DIR}: ${String(error)}`] }
  }

  const idCounts = new Map<string, number>()
  const dedupeCounts = new Map<string, number>()
  for (const source of parsed) {
    idCounts.set(source.task.id, (idCounts.get(source.task.id) ?? 0) + 1)
    const dedupe = source.task.dedupe_key
    if (dedupe) dedupeCounts.set(dedupe, (dedupeCounts.get(dedupe) ?? 0) + 1)
  }

  const valid: TaskSource[] = []
  for (const source of parsed) {
    const issues = [
      ...(idCounts.get(source.task.id)! > 1 ? [`duplicate task.id ${source.task.id}`] : []),
      ...(source.task.dedupe_key && dedupeCounts.get(source.task.dedupe_key)! > 1
        ? [`duplicate task.dedupe_key ${source.task.dedupe_key}`]
        : []),
    ]
    if (!issues.length) {
      valid.push(source)
      continue
    }
    const repairReason = issues.join('; ')
    errors.push(`${source.path}: ${repairReason}`)
    repairs.push({
      key: `task-repair:${source.path}`,
      kind: 'task',
      item_id: source.task.id,
      item_kind: 'task',
      state: 'unsupported',
      title: source.title,
      ...(source.description ? { description: source.description } : {}),
      body: source.body,
      path: source.path,
      created: source.created,
      proofed: false,
      orphan: false,
      relinkCandidates: [],
      relinkMatch: null,
      task: source.task,
      ...(source.generated ? { generatedBy: source.generated.by } : {}),
      repairReason,
    })
  }
  return { sources: valid, repairs, errors }
}

function sameSource(projection: SourceRef, source: IdeaSource): boolean {
  return projection.path === source.path && projection.created === source.created
}

function sourceOwners(projection: LedgerProjection, source: IdeaSource): ReadonlySet<string> {
  return ownersOfSource(projection.sourceToIdeaId, {
    path: source.path,
    ...(source.created ? { created: source.created } : {}),
  })
}

function textOfProjection(projection: IdeaProjection): string {
  const project = projectTagsOf(projection).join(' ')
  switch (projection.state) {
    case 'wip': return `${project} ${projection.commitment} ${projection.next_action} ${projection.close_condition}`
    case 'waiting': return `${project} ${projection.waiting_for} ${projection.review_at}`
    case 'dormant': return `${project} ${projection.wake_trigger} ${projection.next_action ?? ''}`
    case 'closed': return `${project} ${projection.exit.kind} ${projection.exit.via ?? ''} ${projection.reason ?? ''} ${projection.target ?? ''} ${projection.result ?? ''}`
    case 'unsupported': return `${project} ${projection.unsupported_actions.join(' ')}`
    case 'capture': return project
  }
}

export function itemSearchText(item: WorkspaceItem): string {
  const task = item.task
    ? `${item.task.due ?? ''} ${item.task.done_when ?? ''} ${item.task.dedupe_key ?? ''} ${item.generatedBy ?? ''}`
    : ''
  return `${item.title} ${item.body ?? ''} ${item.path ?? ''} ${task} ${item.repairReason ?? ''} ${item.projection ? textOfProjection(item.projection) : ''}`.toLocaleLowerCase()
}

function taskBody(source: TaskSource): string | undefined {
  const sections = [source.description?.trim(), source.body.trim()].filter(Boolean)
  return sections.length ? sections.join('\n\n') : undefined
}

function projectItems(
  sources: IdeaSource[],
  taskSources: TaskSource[],
  projection: LedgerProjection,
): WorkspaceItem[] {
  const byPath = new Map(sources.map((source) => [source.path, source]))
  const tasksById = new Map(taskSources.map((source) => [source.task.id, source]))
  const claimedPaths = new Set(
    sources
      .filter((source) => sourceOwners(projection, source).size > 0)
      .map((source) => source.path),
  )
  const claimedTaskIds = new Set(
    [...projection.items.values()]
      .filter((item) => item.item_kind === 'task')
      .map((item) => item.item_id),
  )
  const projected: WorkspaceItem[] = []

  for (const idea of projection.items.values()) {
    if (idea.item_kind === 'task') {
      const source = tasksById.get(idea.item_id)
      projected.push({
        key: itemKey('task', idea.item_id),
        kind: 'task',
        item_id: idea.item_id,
        item_kind: 'task',
        state: idea.state,
        title: source?.title
          ?? idea.source?.path.split('/').at(-1)?.replace(/-task\.md$/, '')
          ?? idea.item_id,
        ...(source ? { body: taskBody(source), path: source.path, created: source.created, task: source.task } : {}),
        ...(source?.description ? { description: source.description } : {}),
        ...(!source && idea.source?.path ? { path: idea.source.path } : {}),
        ...(!source && idea.source?.created ? { created: idea.source.created } : {}),
        ...(source?.generated ? { generatedBy: source.generated.by } : {}),
        proofed: false,
        orphan: !source,
        projection: idea,
        relinkCandidates: [],
        relinkMatch: null,
      })
      continue
    }
    const source = idea.source ? byPath.get(idea.source.path) : undefined
    const sourceExists = Boolean(source && idea.source && sameSource(idea.source, source))
    const availableSources = sources.filter((candidate) => {
      const owners = sourceOwners(projection, candidate)
      return owners.size === 0 || [...owners].every((owner) => owner === idea.idea_id)
    })
    const createdMatches = idea.source?.created
      ? availableSources.filter((candidate) => candidate.created === idea.source?.created)
      : []
    const relinkCandidates = sourceExists ? [] : createdMatches.length > 0 ? createdMatches : availableSources
    projected.push({
      key: idea.idea_id,
      kind: 'idea',
      item_id: idea.item_id,
      item_kind: 'idea',
      idea_id: idea.idea_id,
      state: idea.state,
      title: sourceExists && source
        ? source.title
        : idea.source?.path.split('/').at(-1)?.replace(/-idea\.md$/, '') ?? idea.idea_id,
      ...(sourceExists && source ? { body: source.body } : {}),
      ...(idea.source?.path ? { path: idea.source.path } : {}),
      ...(idea.source?.created ? { created: idea.source.created } : {}),
      proofed: sourceExists ? source?.proofed ?? false : false,
      orphan: !sourceExists,
      projection: idea,
      relinkCandidates,
      relinkMatch: sourceExists ? null : createdMatches.length > 0 ? 'created' : 'manual',
    })
  }

  const implicit = sources
    .filter((source) => !claimedPaths.has(source.path))
    .map<WorkspaceItem>((source) => ({
      key: source.path,
      kind: 'idea',
      item_kind: 'idea',
      state: 'capture',
      title: source.title,
      body: source.body,
      path: source.path,
      ...(source.created ? { created: source.created } : {}),
      proofed: source.proofed,
      orphan: false,
      relinkCandidates: [],
      relinkMatch: null,
    }))
  const implicitTasks = taskSources
    .filter((source) => !claimedTaskIds.has(source.task.id))
    .map<WorkspaceItem>((source) => ({
      key: itemKey('task', source.task.id),
      kind: 'task',
      item_id: source.task.id,
      item_kind: 'task',
      state: 'capture',
      title: source.title,
      ...(source.description ? { description: source.description } : {}),
      ...(taskBody(source) ? { body: taskBody(source) } : {}),
      path: source.path,
      created: source.created,
      proofed: false,
      orphan: false,
      relinkCandidates: [],
      relinkMatch: null,
      task: source.task,
      ...(source.generated ? { generatedBy: source.generated.by } : {}),
    }))
  return [...projected, ...implicit, ...implicitTasks]
}

function byLastEventDesc(a: WorkspaceItem, b: WorkspaceItem): number {
  const left = a.projection?.last_at ?? a.created ?? ''
  const right = b.projection?.last_at ?? b.created ?? ''
  return right.localeCompare(left)
}

function projectOptionsOf(items: WorkspaceItem[]): string[] {
  const options: string[] = []
  for (const item of items.slice().sort(byLastEventDesc)) {
    const projection = item.projection
    if (!projection) continue
    const values = [
      ...projectTagsOf(projection),
      projection.state === 'closed'
        && projection.exit.kind === 'transferred'
        && projection.exit.via === 'project'
        ? projection.target
        : undefined,
    ]
    for (const value of values) {
      const normalized = value?.trim()
      if (normalized && !options.some((option) => projectTagKey(option) === projectTagKey(normalized))) options.push(normalized)
    }
  }
  return options
}

function withProjectSuggestions(items: WorkspaceItem[], projectOptions: string[]): WorkspaceItem[] {
  const matcher = buildProjectMatcher(
    items.flatMap((item) => {
      const projects = projectTagsOf(item.projection)
      return item.projection && projects.length && item.body?.trim()
        ? [{ projects, text: `${item.title}\n${item.body}` }]
        : []
    }),
    projectOptions,
  )
  return items.map((item) => {
    if (item.state !== 'capture' || item.projection || !item.body?.trim()) return item
    const suggestedProject = matcher.recommend(`${item.title}\n${item.body}`)
    return suggestedProject ? { ...item, suggestedProject } : item
  })
}

async function persistSourceDirs(
  loadedRaw: string | null,
  loadedLedger: LedgerDocument,
  desiredDirs: string[],
  port: VaultPort,
): Promise<{ ledger: LedgerDocument; raw: string }> {
  const existsNow = (await port.exists(NEXT_PATH)).exists
  let currentRaw: string | null = null
  let current = newLedger([])

  if (existsNow) {
    currentRaw = (await port.read(NEXT_PATH)).content
    current = parseLedger(currentRaw)
  } else if (loadedRaw !== null) {
    throw new Error('Next document disappeared while updating source directories')
  } else {
    current = loadedLedger
  }

  const sourceDirs = unique([...current.source_dirs, ...desiredDirs])
  if (currentRaw !== null && sourceDirs.length === current.source_dirs.length) {
    return { ledger: current, raw: currentRaw }
  }

  const next: LedgerDocument = { ...current, source_dirs: sourceDirs }
  const serialized = serializeLedger(next)
  await port.write(NEXT_PATH, serialized)
  const written = (await port.read(NEXT_PATH)).content
  if (written !== serialized) throw new Error('Next source directory snapshot did not match after writing')
  return { ledger: next, raw: serialized }
}

export async function loadWorkspace(port: VaultPort = hostVault): Promise<NextWorkspace> {
  const ideaDir = await currentIdeaDir(port)
  let ledgerRaw: string | null = null
  let ledger = newLedger([ideaDir])
  let readOnlyError: string | null = null

  try {
    if ((await port.exists(NEXT_PATH)).exists) {
      ledgerRaw = (await port.read(NEXT_PATH)).content
      ledger = parseLedger(ledgerRaw)
    }
    const persisted = await persistSourceDirs(ledgerRaw, ledger, [ideaDir], port)
    ledger = persisted.ledger
    ledgerRaw = persisted.raw
  } catch (error) {
    readOnlyError = String(error)
  }

  const sourceDirs = unique([...ledger.source_dirs, ideaDir])
  const [scan, taskScan] = await Promise.all([
    scanIdeaDirs(sourceDirs, port),
    scanTaskDir(port),
  ])
  const projection = reduceEvents(ledger.events)
  const blockedTaskIds = new Set(taskScan.repairs.flatMap((item) => item.item_id ? [item.item_id] : []))
  const blockedTaskPaths = new Set(taskScan.repairs.flatMap((item) => item.path ? [item.path] : []))
  const safeProjectedItems = projectItems(scan.sources, taskScan.sources, projection).filter((item) => !(
    item.kind === 'task'
      && item.projection
      && (Boolean(item.item_id && blockedTaskIds.has(item.item_id))
        || Boolean(item.projection.source?.path && blockedTaskPaths.has(item.projection.source.path)))
  ))
  const projectedItems = [...safeProjectedItems, ...taskScan.repairs]
  const projectOptions = projectOptionsOf(projectedItems)
  const items = withProjectSuggestions(projectedItems, projectOptions)
  return {
    ledger,
    ledgerRaw,
    sourceDirs,
    ideaDir,
    taskDir: DEFAULT_TASK_DIR,
    projection,
    sources: scan.sources,
    taskSources: taskScan.sources,
    items,
    capture: items.filter((item) => item.state === 'capture' && !item.orphan).sort(byLastEventDesc),
    wip: items.filter((item) => item.state === 'wip').sort(byLastEventDesc),
    waiting: items.filter((item) => item.state === 'waiting').sort(byLastEventDesc),
    dormant: items.filter((item) => item.state === 'dormant').sort(byLastEventDesc),
    closed: items.filter((item) => item.state === 'closed').sort(byLastEventDesc),
    unsupported: items.filter((item) => item.state === 'unsupported').sort(byLastEventDesc),
    projectOptions,
    scanErrors: [...scan.errors, ...taskScan.errors],
    readOnlyError,
  }
}

function toRecord(event: NextEvent): Record<string, unknown> {
  return event as unknown as Record<string, unknown>
}

export async function appendEvent(
  loaded: NextWorkspace,
  event: NextEvent,
  options: { hardWipLimit?: boolean } = {},
  port: VaultPort = hostVault,
): Promise<NextWorkspace> {
  if (loaded.readOnlyError) throw new NextWriteError('read_only', loaded.readOnlyError)

  const existsNow = (await port.exists(NEXT_PATH)).exists
  if (!existsNow && loaded.ledgerRaw !== null) {
    throw new NextWriteError('changed_missing', 'Next document disappeared after it was loaded')
  }

  let currentRaw: string | null = null
  let current = newLedger(loaded.sourceDirs)
  if (existsNow) {
    currentRaw = (await port.read(NEXT_PATH)).content
    // Always parse the final read. A malformed or newer document is never replaced.
    current = parseLedger(currentRaw)
  }

  // Exact byte comparison has no hash-collision risk. A changed document is
  // not rejected by itself: the refreshed projection below decides whether
  // the same event is still valid, preserving unrelated external additions.
  const changedSinceLoad = currentRaw !== loaded.ledgerRaw
  const sourceDirs = unique([...current.source_dirs, ...loaded.sourceDirs])
  const projection = reduceEvents(current.events)
  const targetVersion = event.item_kind === 'task' ? 2 : current.version
  const validation = validateAppend(projection, event, {
    hardLimit: options.hardWipLimit,
    ledgerVersion: targetVersion,
  })
  if (!validation.ok) {
    const detail = validation.issues.map((issue) => issue.message).join('; ')
    throw new NextWriteError('invalid_event', changedSinceLoad ? `Next changed: ${detail}` : detail)
  }
  if (validation.idempotent) return loadWorkspace(port)

  const writable = event.item_kind === 'task'
    ? upgradeLedgerToV2(current, [DEFAULT_TASK_DIR])
    : current
  const next: LedgerDocument = {
    ...writable,
    source_dirs: sourceDirs,
    events: [...writable.events, toRecord(event)],
  }
  const serialized = serializeLedger(next)
  await port.write(NEXT_PATH, serialized)
  const written = (await port.read(NEXT_PATH)).content
  if (written !== serialized) {
    throw new NextWriteError('write_verification_failed', 'Next document did not match after writing')
  }
  return loadWorkspace(port)
}

export async function openSource(item: WorkspaceItem, port: VaultPort = hostVault): Promise<void> {
  if (!item.path || item.orphan) throw new Error('source file is missing')
  await port.open(item.path)
}

export function sourceRefOf(item: WorkspaceItem): SourceRef | undefined {
  if (item.projection?.source) return item.projection.source
  if (!item.path) return undefined
  return { path: item.path, ...(item.created ? { created: item.created } : {}) }
}
