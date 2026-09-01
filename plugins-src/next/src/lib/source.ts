import { isMap, isScalar } from 'yaml'
import YAML from 'yaml'
import { editableFrontmatter, serializeEditableFrontmatter } from './frontmatter-document'
import { DEFAULT_PRIORITY, normalizeContexts, normalizeDue, normalizePriority, type PlanningMetadata, type Priority } from './metadata'

export const IDEA_SUFFIX = '-idea.md'
export const IDEA_SPARK_STATE_PATH = '.notemd/idea-spark.json'
export const DEFAULT_IDEA_DIR = 'inbox/ideas'

export interface IdeaSource {
  path: string
  created?: string
  title: string
  /** Human-authored Markdown after frontmatter, preserved verbatim for preview. */
  body: string
  proofed: boolean
  priority?: Priority
  due?: string
  contexts?: string[]
}

export function normalizeVaultDir(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const normalized = value.trim().replace(/\\/g, '/').replace(/\/+$/g, '')
  if (!normalized || normalized.startsWith('/') || normalized.includes('\0')) return null
  const segments = normalized.split('/')
  if (segments.some((segment) => !segment || segment === '.' || segment === '..')) return null
  return normalized
}

export function parseIdeaDir(raw: string | null): string {
  if (!raw) return DEFAULT_IDEA_DIR
  try {
    const value = JSON.parse(raw) as { ideaDir?: unknown }
    return normalizeVaultDir(value?.ideaDir) ?? DEFAULT_IDEA_DIR
  } catch {
    return DEFAULT_IDEA_DIR
  }
}

export function isIdeaFileName(name: string): boolean {
  return name.endsWith(IDEA_SUFFIX) && !name.endsWith('.proof.md')
}

export function proofPathFor(path: string): string {
  return path.endsWith('.md') ? `${path.slice(0, -3)}.proof.md` : `${path}.proof.md`
}

/** Idea Spark-compatible local-time name; proof sidecars reserve the slot too. */
export function timestampIdeaFileName(now: Date, taken: ReadonlySet<string>): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  const base = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`
  let name = `${base}${IDEA_SUFFIX}`
  let suffix = 2
  while (taken.has(name) || taken.has(proofPathFor(name))) {
    name = `${base}-${suffix}${IDEA_SUFFIX}`
    suffix += 1
  }
  return name
}

/** Minimal OKF Idea document. The human-authored body is preserved verbatim. */
export function buildIdeaDocument(
  body: string,
  created: string,
  metadata: PlanningMetadata = { priority: DEFAULT_PRIORITY, contexts: [] },
): string {
  const contexts = normalizeContexts(metadata.contexts)
  const due = normalizeDue(metadata.due)
  const planning = [
    `  priority: ${normalizePriority(metadata.priority)}`,
    ...(due ? [`  due: ${JSON.stringify(due)}`] : []),
    ...(contexts.length
      ? ['  contexts:', ...contexts.map((context) => `    - ${JSON.stringify(context)}`)]
      : []),
  ].join('\n')
  return `---\ntype: Idea\ncreated: ${created}\nnext:\n${planning}\n---\n${body}`
}

export function splitFrontmatter(markdown: string): [Record<string, unknown> | null, string] {
  const lines = markdown.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') return [null, markdown]
  const end = lines.findIndex((line, index) => index > 0 && line.trim() === '---')
  if (end < 0) return [null, markdown]
  let parsed: unknown
  try {
    parsed = YAML.parse(lines.slice(1, end).join('\n'))
  } catch {
    parsed = null
  }
  const meta = parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)
    ? parsed as Record<string, unknown>
    : null
  return [meta, lines.slice(end + 1).join('\n')]
}

export function titleFromMarkdown(markdown: string, fallback: string): string {
  const [meta, body] = splitFrontmatter(markdown)
  if (typeof meta?.title === 'string' && meta.title.trim()) return meta.title.trim()
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim()
    if (!line) continue
    const title = line
      .replace(/^\s{0,3}(?:#{1,6}(?:\s+|$)|>\s*|[-*+]\s+|\d+[.)]\s+)/, '')
      .trim()
    if (title) return title
  }
  return fallback.replace(IDEA_SUFFIX, '')
}

export function parseIdeaSource(path: string, markdown: string, proofed: boolean): IdeaSource {
  const [meta, body] = splitFrontmatter(markdown)
  const created = typeof meta?.created === 'string' && meta.created.trim() ? meta.created.trim() : undefined
  const planning = meta?.next !== null && typeof meta?.next === 'object' && !Array.isArray(meta.next)
    ? meta.next as Record<string, unknown>
    : null
  const priority = planning && typeof planning.priority === 'string'
    && normalizePriority(planning.priority) === planning.priority
    ? normalizePriority(planning.priority)
    : undefined
  const due = normalizeDue(planning?.due)
  const contexts = normalizeContexts(planning?.contexts)
  const name = path.split('/').at(-1) ?? path
  return {
    path,
    ...(created ? { created } : {}),
    title: titleFromMarkdown(markdown, name),
    body,
    proofed,
    ...(priority ? { priority } : {}),
    ...(due ? { due } : {}),
    ...(contexts.length ? { contexts } : {}),
  }
}

/** Update plugin-owned Idea planning fields without touching unrelated metadata or body bytes. */
export function updateIdeaPlanningDocument(markdown: string, metadata: PlanningMetadata): string {
  const editable = editableFrontmatter(markdown, true)
  const { document } = editable
  if (document.get('type') === undefined) document.set('type', 'Idea')
  if (!isMap(document.get('next', true))) document.set('next', document.createNode({}))
  document.setIn(['next', 'priority'], normalizePriority(metadata.priority))
  const due = normalizeDue(metadata.due)
  if (due) {
    document.setIn(['next', 'due'], due)
    const dueNode = document.getIn(['next', 'due'], true)
    if (isScalar(dueNode)) dueNode.type = 'QUOTE_DOUBLE'
  } else {
    document.deleteIn(['next', 'due'])
  }
  const contexts = normalizeContexts(metadata.contexts)
  if (contexts.length) document.setIn(['next', 'contexts'], contexts)
  else document.deleteIn(['next', 'contexts'])
  return serializeEditableFrontmatter(editable)
}

export function sortIdeasNewestFirst(items: IdeaSource[]): IdeaSource[] {
  return items.slice().sort((a, b) => {
    const aTime = a.created ? Date.parse(a.created) : Number.NaN
    const bTime = b.created ? Date.parse(b.created) : Number.NaN
    if (Number.isFinite(aTime) && Number.isFinite(bTime) && aTime !== bTime) return bTime - aTime
    if (Number.isFinite(aTime) !== Number.isFinite(bTime)) return Number.isFinite(bTime) ? 1 : -1
    return b.path.localeCompare(a.path)
  })
}
