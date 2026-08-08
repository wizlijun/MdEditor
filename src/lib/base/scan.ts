import { parse } from 'yaml'
import { joinPath, dirname as parentDir } from '../paths'
import type { FileRecord } from './model'

// `paths.ts` (not folder-view.svelte.ts) so this module stays free of
// Tauri/runes imports and scan.test.ts runs hermetically.

export interface ScanDeps {
  readDir: (dir: string) => Promise<{ name: string; isDirectory: boolean }[]>
  stat: (path: string) => Promise<{ mtime?: Date | null; birthtime?: Date | null; size?: number } | null>
  readTextFile: (path: string) => Promise<string>
}

const FM_RE = /^---\r?\n([\s\S]*?)\r?\n---/

function normalizeTags(v: unknown): string[] {
  if (Array.isArray(v)) return v.filter((x): x is string => typeof x === 'string')
  if (typeof v === 'string') return [v]
  return []
}

/** Extract leading YAML frontmatter into an object + normalized tags. */
export function extractFrontmatter(text: string): { data: Record<string, unknown>; tags: string[] } {
  const m = FM_RE.exec(text)
  if (!m) return { data: {}, tags: [] }
  let data: Record<string, unknown> = {}
  try {
    const parsed = parse(m[1])
    if (parsed && typeof parsed === 'object') data = parsed as Record<string, unknown>
  } catch {
    data = {}
  }
  return { data, tags: normalizeTags(data.tags) }
}

// Companion "sidecar" notes (xxx.note.md / xxx.notes.md) are excluded: a .base
// table shows primary documents, not their paired notes. Same suffix as folder-view.
const SIDECAR_RE = /\.notes?\.md$/i
const isMd = (name: string) => /\.md$/i.test(name) && !SIDECAR_RE.test(name) && !name.startsWith('.')

/** Recursively scan `dir` for markdown files → FileRecord[]. */
export async function scanBaseDir(dir: string, deps: ScanDeps): Promise<FileRecord[]> {
  const out: FileRecord[] = []
  const walk = async (d: string): Promise<void> => {
    const entries = await deps.readDir(d).catch(() => [])
    await Promise.all(entries.map(async (e) => {
      if (e.name.startsWith('.')) return
      const path = joinPath(d, e.name)
      if (e.isDirectory) return walk(path)
      if (!isMd(e.name)) return
      const [st, text] = await Promise.all([
        deps.stat(path).catch(() => null),
        deps.readTextFile(path).catch(() => ''),
      ])
      const { data, tags } = extractFrontmatter(text)
      out.push({
        path,
        name: e.name,
        folder: parentDir(path),
        ext: 'md',
        mtime: st?.mtime ? new Date(st.mtime).getTime() : 0,
        ctime: st?.birthtime ? new Date(st.birthtime).getTime() : 0,
        size: st?.size ?? 0,
        tags,
        frontmatter: data,
      })
    }))
  }
  await walk(dir)
  return out
}
