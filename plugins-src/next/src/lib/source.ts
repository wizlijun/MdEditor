import YAML from 'yaml'

export const IDEA_SUFFIX = '-idea.md'
export const IDEA_SPARK_STATE_PATH = '.notemd/idea-spark.json'
export const DEFAULT_IDEA_DIR = 'inbox/ideas'

export interface IdeaSource {
  path: string
  created?: string
  title: string
  proofed: boolean
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
  const [meta] = splitFrontmatter(markdown)
  const created = typeof meta?.created === 'string' && meta.created.trim() ? meta.created.trim() : undefined
  const name = path.split('/').at(-1) ?? path
  return {
    path,
    ...(created ? { created } : {}),
    title: titleFromMarkdown(markdown, name),
    proofed,
  }
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
