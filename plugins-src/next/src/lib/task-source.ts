import { Document, isScalar, parseDocument } from 'yaml'
import { editableFrontmatter, serializeEditableFrontmatter } from './frontmatter-document'
import { normalizeContexts, normalizePriority, PRIORITIES, type Priority } from './metadata'
import type { PlanningMetadata } from './metadata'

export const TASK_SUFFIX = '-task.md'
export const DEFAULT_TASK_DIR = 'inbox/tasks'

export interface TaskDetails {
  version: 1
  id: string
  /** Source-level project affiliation; it does not confirm a Next ledger tag. */
  project?: string
  priority?: Priority
  due?: string
  contexts?: string[]
  done_when?: string
  dedupe_key?: string
}

export interface TaskGenerated {
  by: string
  at: string
}

export interface TaskReference {
  resource: string
  id?: string
  title?: string
}

export interface TaskSource {
  path: string
  title: string
  description?: string
  created: string
  task: TaskDetails
  generated?: TaskGenerated
  sources?: TaskReference[]
  /** Markdown after the closing frontmatter delimiter, preserved byte-for-byte. */
  body: string
  /** Complete parsed mapping, including fields introduced by future producers. */
  frontmatter: Readonly<Record<string, unknown>>
}

export interface TaskIdentityHint {
  id?: string
  title?: string
}

export interface BuildTaskDocumentInput {
  title: string
  description?: string
  created: string
  task: TaskDetails
  generated?: TaskGenerated
  sources?: TaskReference[]
  body?: string
}

export type TaskSourceErrorCode =
  | 'invalid-path'
  | 'missing-frontmatter'
  | 'invalid-frontmatter'
  | 'invalid-type'
  | 'invalid-title'
  | 'invalid-description'
  | 'invalid-created'
  | 'invalid-task'
  | 'unsupported-version'
  | 'invalid-task-id'
  | 'invalid-project'
  | 'invalid-priority'
  | 'invalid-due'
  | 'invalid-contexts'
  | 'invalid-done-when'
  | 'invalid-dedupe-key'
  | 'invalid-generated'
  | 'invalid-sources'

export class TaskSourceError extends Error {
  constructor(
    public readonly code: TaskSourceErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'TaskSourceError'
  }
}

function fail(code: TaskSourceErrorCode, message: string): never {
  throw new TaskSourceError(code, message)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function requiredString(
  value: unknown,
  code: TaskSourceErrorCode,
  label: string,
): string {
  if (typeof value !== 'string' || !value.trim()) fail(code, `${label} must be a non-empty string`)
  return value.trim()
}

function optionalString(
  value: unknown,
  code: TaskSourceErrorCode,
  label: string,
): string | undefined {
  if (value === undefined) return undefined
  return requiredString(value, code, label)
}

function validCalendarDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value)
  if (!match) return false
  const year = Number(match[1])
  const month = Number(match[2])
  const day = Number(match[3])
  if (month < 1 || month > 12 || day < 1) return false
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
  return day <= (days[month - 1] ?? 0)
}

/** RFC 3339 datetime with an explicit Z or numeric offset. */
function validRfc3339(value: string): boolean {
  const match = /^(\d{4}-\d{2}-\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-](\d{2}):(\d{2}))$/i.exec(value)
  if (!match || !validCalendarDate(match[1] ?? '')) return false
  const hour = Number(match[2])
  const minute = Number(match[3])
  const second = Number(match[4])
  const offsetHour = match[5] === undefined ? 0 : Number(match[5])
  const offsetMinute = match[6] === undefined ? 0 : Number(match[6])
  return hour <= 23 && minute <= 59 && second <= 60 && offsetHour <= 23 && offsetMinute <= 59
}

function validUuidV4(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)
}

interface ParsedTaskFrontmatter {
  value: Record<string, unknown>
  body: string
  dueIsQuoted: boolean
}

function splitTaskFrontmatter(markdown: string): ParsedTaskFrontmatter {
  let start: number
  if (markdown.startsWith('---\n')) start = 4
  else if (markdown.startsWith('---\r\n')) start = 5
  else fail('missing-frontmatter', 'Task document must start with YAML frontmatter')

  const closing = /^---[ \t]*(?:\r?\n|$)/gm
  closing.lastIndex = start
  const match = closing.exec(markdown)
  if (!match) fail('missing-frontmatter', 'Task document frontmatter is not closed')

  const document = parseDocument(markdown.slice(start, match.index), { prettyErrors: false })
  if (document.errors.length > 0) {
    fail('invalid-frontmatter', `Task frontmatter is invalid YAML: ${document.errors[0]?.message ?? 'unknown error'}`)
  }
  const parsed: unknown = document.toJS()
  if (!isRecord(parsed)) fail('invalid-frontmatter', 'Task frontmatter must be a mapping')
  const dueNode = document.getIn(['task', 'due'], true)
  const dueIsQuoted = dueNode === undefined
    || (isScalar(dueNode) && (dueNode.type === 'QUOTE_DOUBLE' || dueNode.type === 'QUOTE_SINGLE'))
  return {
    value: parsed,
    body: markdown.slice(match.index + match[0].length),
    dueIsQuoted,
  }
}

interface ValidatedTaskMeta {
  title: string
  description?: string
  created: string
  task: TaskDetails
  generated?: TaskGenerated
  sources?: TaskReference[]
}

function validateTaskMeta(meta: Record<string, unknown>, dueIsQuoted = true): ValidatedTaskMeta {
  if (meta.type !== 'Task') fail('invalid-type', 'type must be Task')
  const title = requiredString(meta.title, 'invalid-title', 'title')
  const description = optionalString(meta.description, 'invalid-description', 'description')
  const created = requiredString(meta.created, 'invalid-created', 'created')
  if (!validRfc3339(created)) fail('invalid-created', 'created must be an RFC 3339 datetime with timezone')

  if (!isRecord(meta.task)) fail('invalid-task', 'task must be a mapping')
  if (meta.task.version !== 1) {
    if (typeof meta.task.version === 'number' && meta.task.version > 1) {
      fail('unsupported-version', `task.version ${meta.task.version} is not supported`)
    }
    fail('invalid-task', 'task.version must be 1')
  }
  const id = requiredString(meta.task.id, 'invalid-task-id', 'task.id')
  if (!validUuidV4(id)) fail('invalid-task-id', 'task.id must be a UUID v4')
  const project = optionalString(meta.task.project, 'invalid-project', 'task.project')
  let priority: Priority | undefined
  if (meta.task.priority !== undefined) {
    if (typeof meta.task.priority !== 'string' || !(PRIORITIES as readonly string[]).includes(meta.task.priority)) {
      fail('invalid-priority', 'task.priority must be P0, P1, P2, or P3')
    }
    priority = normalizePriority(meta.task.priority)
  }
  const due = optionalString(meta.task.due, 'invalid-due', 'task.due')
  if (due !== undefined && !validCalendarDate(due)) fail('invalid-due', 'task.due must be YYYY-MM-DD')
  if (due !== undefined && !dueIsQuoted) fail('invalid-due', 'task.due must be a quoted YYYY-MM-DD string')
  let contexts: string[] | undefined
  if (meta.task.contexts !== undefined) {
    if (!Array.isArray(meta.task.contexts)
      || meta.task.contexts.some((context) => typeof context !== 'string' || !context.trim())) {
      fail('invalid-contexts', 'task.contexts must be a sequence of non-empty strings')
    }
    contexts = normalizeContexts(meta.task.contexts)
    if (contexts.length !== meta.task.contexts.length) {
      fail('invalid-contexts', 'task.contexts must not contain duplicates')
    }
  }
  const doneWhen = optionalString(meta.task.done_when, 'invalid-done-when', 'task.done_when')
  const dedupeKey = optionalString(meta.task.dedupe_key, 'invalid-dedupe-key', 'task.dedupe_key')
  const task: TaskDetails = {
    version: 1,
    id,
    ...(project ? { project } : {}),
    ...(priority ? { priority } : {}),
    ...(due ? { due } : {}),
    ...(contexts?.length ? { contexts } : {}),
    ...(doneWhen ? { done_when: doneWhen } : {}),
    ...(dedupeKey ? { dedupe_key: dedupeKey } : {}),
  }

  let generated: TaskGenerated | undefined
  if (meta.generated !== undefined) {
    if (!isRecord(meta.generated)) fail('invalid-generated', 'generated must be a mapping')
    const by = requiredString(meta.generated.by, 'invalid-generated', 'generated.by')
    const at = requiredString(meta.generated.at, 'invalid-generated', 'generated.at')
    if (!/^[^\s/]+\/[^\s/]+$/.test(by)) {
      fail('invalid-generated', 'generated.by must use producer/version format')
    }
    if (!validRfc3339(at)) fail('invalid-generated', 'generated.at must be an RFC 3339 datetime with timezone')
    generated = { by, at }
  }

  let sources: TaskReference[] | undefined
  if (meta.sources !== undefined) {
    if (!Array.isArray(meta.sources)) fail('invalid-sources', 'sources must be a sequence')
    sources = meta.sources.map((source, index) => {
      if (!isRecord(source)) fail('invalid-sources', `sources[${index}] must be a mapping`)
      const resource = requiredString(source.resource, 'invalid-sources', `sources[${index}].resource`)
      const sourceId = optionalString(source.id, 'invalid-sources', `sources[${index}].id`)
      const sourceTitle = optionalString(source.title, 'invalid-sources', `sources[${index}].title`)
      return {
        resource,
        ...(sourceId ? { id: sourceId } : {}),
        ...(sourceTitle ? { title: sourceTitle } : {}),
      }
    })
  }
  if (generated && !dedupeKey) {
    fail('invalid-dedupe-key', 'Agent-generated Tasks require task.dedupe_key')
  }
  if (generated && (!sources || sources.length === 0)) {
    fail('invalid-sources', 'Agent-generated Tasks require at least one sources[].resource')
  }

  return {
    title,
    ...(description ? { description } : {}),
    created,
    task,
    ...(generated ? { generated } : {}),
    ...(sources ? { sources } : {}),
  }
}

export function isTaskFileName(name: string): boolean {
  return !name.includes('/') && !name.includes('\\') && name.length > TASK_SUFFIX.length && name.endsWith(TASK_SUFFIX)
}

/** Task discovery is intentionally fixed to direct files in inbox/tasks. */
export function isTaskPath(path: string): boolean {
  const prefix = `${DEFAULT_TASK_DIR}/`
  if (!path.startsWith(prefix)) return false
  return isTaskFileName(path.slice(prefix.length))
}

export function parseTaskSource(path: string, markdown: string): TaskSource {
  if (!isTaskPath(path)) fail('invalid-path', `Task path must be a direct ${DEFAULT_TASK_DIR}/*${TASK_SUFFIX} file`)
  const parsed = splitTaskFrontmatter(markdown)
  const validated = validateTaskMeta(parsed.value, parsed.dueIsQuoted)
  return { path, ...validated, body: parsed.body, frontmatter: parsed.value }
}

/** Update plugin-owned Task planning fields and revalidate the complete source contract. */
export function updateTaskPlanningDocument(
  path: string,
  markdown: string,
  metadata: PlanningMetadata,
): string {
  // Refuse to repair or reinterpret an invalid/future Task through this editor.
  parseTaskSource(path, markdown)
  const editable = editableFrontmatter(markdown, false)
  const { document } = editable
  document.setIn(['task', 'priority'], normalizePriority(metadata.priority))
  if (metadata.due) {
    document.setIn(['task', 'due'], metadata.due)
    const due = document.getIn(['task', 'due'], true)
    if (isScalar(due)) due.type = 'QUOTE_DOUBLE'
  } else {
    document.deleteIn(['task', 'due'])
  }
  const contexts = normalizeContexts(metadata.contexts)
  if (contexts.length) document.setIn(['task', 'contexts'], contexts)
  else document.deleteIn(['task', 'contexts'])

  const updated = serializeEditableFrontmatter(editable)
  parseTaskSource(path, updated)
  return updated
}

/**
 * Best-effort identity for a repair card. This never makes an invalid Task
 * readable; it only lets the repository quarantine an existing projection
 * whose source has become malformed or moved to a future schema version.
 */
export function taskIdentityHint(markdown: string): TaskIdentityHint {
  try {
    const meta = splitTaskFrontmatter(markdown).value
    const title = typeof meta.title === 'string' && meta.title.trim() ? meta.title.trim() : undefined
    const id = isRecord(meta.task)
      && typeof meta.task.id === 'string'
      && validUuidV4(meta.task.id.trim())
      ? meta.task.id.trim()
      : undefined
    return { ...(id ? { id } : {}), ...(title ? { title } : {}) }
  } catch {
    return {}
  }
}

/** Build one complete Task file; callers still own create-only publication. */
export function buildTaskDocument(input: BuildTaskDocumentInput): string {
  const frontmatter: Record<string, unknown> = {
    type: 'Task',
    title: input.title.trim(),
    ...(input.description !== undefined ? { description: input.description.trim() } : {}),
    created: input.created.trim(),
    task: {
      version: input.task.version,
      id: input.task.id.trim(),
      ...(input.task.project !== undefined ? { project: input.task.project.trim() } : {}),
      ...(input.task.priority !== undefined ? { priority: input.task.priority } : {}),
      ...(input.task.due !== undefined ? { due: input.task.due.trim() } : {}),
      ...(input.task.contexts?.length ? { contexts: normalizeContexts(input.task.contexts) } : {}),
      ...(input.task.done_when !== undefined ? { done_when: input.task.done_when.trim() } : {}),
      ...(input.task.dedupe_key !== undefined ? { dedupe_key: input.task.dedupe_key.trim() } : {}),
    },
    ...(input.generated ? {
      generated: { by: input.generated.by.trim(), at: input.generated.at.trim() },
    } : {}),
    ...(input.sources ? {
      sources: input.sources.map((source) => ({
        resource: source.resource.trim(),
        ...(source.id !== undefined ? { id: source.id.trim() } : {}),
        ...(source.title !== undefined ? { title: source.title.trim() } : {}),
      })),
    } : {}),
  }
  validateTaskMeta(frontmatter)

  const document = new Document(frontmatter)
  const due = document.getIn(['task', 'due'], true)
  if (isScalar(due)) due.type = 'QUOTE_DOUBLE'
  const yaml = document.toString().trimEnd()
  return `---\n${yaml}\n---\n${input.body ?? ''}`
}

/** Cross-platform-safe slug; code-point truncation never splits a surrogate pair. */
export function taskSlug(title: string): string {
  const normalized = title
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[\p{Cc}\p{Cf}]/gu, '')
    .replace(/[<>:"/\\|?*]/g, '')
    .trim()
    .replace(/\s+/gu, '-')
    .replace(/-+/g, '-')
    .replace(/^[.-]+|[. -]+$/g, '')
  const bounded = [...normalized].slice(0, 48).join('').replace(/[. -]+$/g, '')
  return bounded || 'task'
}

/** Local-time file name. `taken` contains direct child names, not Vault paths. */
export function timestampTaskFileName(now: Date, title: string, taken: ReadonlySet<string>): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  const prefix = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}-${taskSlug(title)}`
  const occupied = new Set([...taken].map((name) => name.toLowerCase()))
  let name = `${prefix}${TASK_SUFFIX}`
  let suffix = 2
  while (occupied.has(name.toLowerCase())) {
    name = `${prefix}-${suffix}${TASK_SUFFIX}`
    suffix += 1
  }
  return name
}

export function findTaskByDedupeKey(
  sources: readonly TaskSource[],
  dedupeKey: string,
): TaskSource | undefined {
  return sources.find((source) => source.task.dedupe_key === dedupeKey)
}
