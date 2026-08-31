import {
  editorOpen,
  vaultExists,
  vaultList,
  vaultRead,
  vaultWrite,
  type VaultEntry,
} from './bridge'
import { ownersOfSource, reduceEvents, validateAppend } from './domain'
import { NEXT_PATH, newLedger, parseLedger, serializeLedger, type LedgerDocument } from './ledger'
import type { IdeaProjection, LedgerProjection, NextEvent, SourceRef } from './model'
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

export interface VaultPort {
  exists(path: string): Promise<{ exists: boolean }>
  list(path: string): Promise<{ entries: VaultEntry[] }>
  read(path: string): Promise<{ content: string }>
  write(path: string, content: string): Promise<{ ok: true }>
  open(path: string): Promise<unknown>
}

export const hostVault: VaultPort = {
  exists: vaultExists,
  list: vaultList,
  read: vaultRead,
  write: vaultWrite,
  open: editorOpen,
}

export interface WorkspaceItem {
  key: string
  idea_id?: string
  state: IdeaProjection['state'] | 'capture'
  title: string
  body?: string
  path?: string
  created?: string
  proofed: boolean
  orphan: boolean
  projection?: IdeaProjection
  relinkCandidates: IdeaSource[]
  relinkMatch: 'created' | 'manual' | null
}

export interface NextWorkspace {
  ledger: LedgerDocument
  ledgerRaw: string | null
  sourceDirs: string[]
  /** Idea Spark's current capture directory; historic sourceDirs remain separate. */
  ideaDir: string
  projection: LedgerProjection
  sources: IdeaSource[]
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

interface ScanResult {
  sources: IdeaSource[]
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
  const project = projection.project ?? ''
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
  return `${item.title} ${item.path ?? ''} ${item.projection ? textOfProjection(item.projection) : ''}`.toLocaleLowerCase()
}

function projectItems(sources: IdeaSource[], projection: LedgerProjection): WorkspaceItem[] {
  const byPath = new Map(sources.map((source) => [source.path, source]))
  const claimedPaths = new Set(
    sources
      .filter((source) => sourceOwners(projection, source).size > 0)
      .map((source) => source.path),
  )
  const projected: WorkspaceItem[] = []

  for (const idea of projection.ideas.values()) {
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
  return [...projected, ...implicit]
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
      projection.project,
      projection.state === 'closed'
        && projection.exit.kind === 'transferred'
        && projection.exit.via === 'project'
        ? projection.target
        : undefined,
    ]
    for (const value of values) {
      const normalized = value?.trim()
      if (normalized && !options.includes(normalized)) options.push(normalized)
    }
  }
  return options
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
  const scan = await scanIdeaDirs(sourceDirs, port)
  const projection = reduceEvents(ledger.events)
  const items = projectItems(scan.sources, projection)
  return {
    ledger,
    ledgerRaw,
    sourceDirs,
    ideaDir,
    projection,
    sources: scan.sources,
    items,
    capture: items.filter((item) => item.state === 'capture' && !item.orphan).sort(byLastEventDesc),
    wip: items.filter((item) => item.state === 'wip').sort(byLastEventDesc),
    waiting: items.filter((item) => item.state === 'waiting').sort(byLastEventDesc),
    dormant: items.filter((item) => item.state === 'dormant').sort(byLastEventDesc),
    closed: items.filter((item) => item.state === 'closed').sort(byLastEventDesc),
    unsupported: items.filter((item) => item.state === 'unsupported').sort(byLastEventDesc),
    projectOptions: projectOptionsOf(items),
    scanErrors: scan.errors,
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
  const validation = validateAppend(projection, event, { hardLimit: options.hardWipLimit })
  if (!validation.ok) {
    const detail = validation.issues.map((issue) => issue.message).join('; ')
    throw new NextWriteError('invalid_event', changedSinceLoad ? `Next changed: ${detail}` : detail)
  }
  if (validation.idempotent) return loadWorkspace(port)

  const next: LedgerDocument = {
    ...current,
    source_dirs: sourceDirs,
    events: [...current.events, toRecord(event)],
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
