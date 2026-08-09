import { readTextFile, writeTextFile, stat as fsStat } from '@tauri-apps/plugin-fs'
import { platform } from './platform.svelte'
import { basename, joinPath } from './paths'

const IOS_LARGE_FILE_LIMIT = 4 * 1024 * 1024

export async function readMd(path: string): Promise<string> {
  if ((await platform().catch(() => 'unknown')) === 'ios') {
    const info = await fsStat(path).catch(() => null)
    if (info && (info.size ?? 0) > IOS_LARGE_FILE_LIMIT) {
      throw new Error(`File too large to open on iOS (limit 4 MB): ${path}`)
    }
  }
  return readTextFile(path)
}

export async function writeMd(path: string, content: string): Promise<void> {
  return writeTextFile(path, content)
}

/**
 * True when a filesystem error stems from a missing access permission rather
 * than a bad path. Covers both tauri-plugin-fs scope rejections ("forbidden
 * path: …") and OS-level denials (macOS TCC / Unix EACCES / EPERM). Callers
 * use this to offer a "grant permission and retry" flow instead of a raw error.
 */
export function isPermissionError(e: unknown): boolean {
  const msg = (e instanceof Error ? e.message : String(e ?? '')).toLowerCase()
  return (
    msg.includes('forbidden path') ||
    msg.includes('not allowed on the scope') ||
    msg.includes('permission denied') ||
    msg.includes('operation not permitted') ||
    msg.includes('access is denied')
  )
}

// Sourced from `paths.ts` so every consumer of `fs.ts` gets the
// separator-agnostic implementations (these two used to split on '/' only,
// which returns the whole path as its own basename on Windows).
// Imported *and* re-exported: `classifyPath` below calls `basename` locally,
// and a bare `export … from` would not bind it in this module's scope.
export { basename, joinPath }

export type FileKind = 'markdown' | 'html' | 'code' | 'image' | 'spreadsheet' | 'base' | 'custom'

export interface FileClass {
  kind: FileKind
  language?: string
}

const EXT_TABLE: Record<string, FileClass> = {
  md:        { kind: 'markdown' },
  markdown:  { kind: 'markdown' },
  mdown:     { kind: 'markdown' },
  mkd:       { kind: 'markdown' },

  html:      { kind: 'html' },
  htm:       { kind: 'html' },

  // Image files (rendered as <img> preview; not editable as text)
  avif:      { kind: 'image' },
  bmp:       { kind: 'image' },
  gif:       { kind: 'image' },
  heic:      { kind: 'image' },
  heif:      { kind: 'image' },
  jpg:       { kind: 'image' },
  jpeg:      { kind: 'image' },
  png:       { kind: 'image' },
  svg:       { kind: 'image' },
  webp:      { kind: 'image' },

  txt:       { kind: 'code', language: '' },
  text:      { kind: 'code', language: '' },
  log:       { kind: 'code', language: '' },
  csv:       { kind: 'spreadsheet' },
  base:      { kind: 'base' },
  tsv:       { kind: 'code', language: '' },
  env:       { kind: 'code', language: '' },

  // Subtitles (plain-text, no hljs grammar → rendered as plaintext)
  srt:       { kind: 'code', language: '' },
  vtt:       { kind: 'code', language: '' },
  ass:       { kind: 'code', language: '' },
  ssa:       { kind: 'code', language: '' },

  // Lightweight markup / prose
  rst:       { kind: 'code', language: '' },
  org:       { kind: 'code', language: '' },
  adoc:      { kind: 'code', language: '' },
  asciidoc:  { kind: 'code', language: '' },
  tex:       { kind: 'code', language: 'latex' },

  // Diffs / property files
  diff:      { kind: 'code', language: 'diff' },
  patch:     { kind: 'code', language: 'diff' },
  properties:{ kind: 'code', language: 'ini' },

  json:      { kind: 'code', language: 'json' },
  jsonc:     { kind: 'code', language: 'json' },
  yaml:      { kind: 'code', language: 'yaml' },
  yml:       { kind: 'code', language: 'yaml' },
  toml:      { kind: 'code', language: 'ini' },     // hljs has no toml; ini is closest available
  ini:       { kind: 'code', language: 'ini' },
  conf:      { kind: 'code', language: 'ini' },
  xml:       { kind: 'code', language: 'xml' },

  sh:        { kind: 'code', language: 'bash' },
  bash:      { kind: 'code', language: 'bash' },
  zsh:       { kind: 'code', language: 'bash' },

  py:        { kind: 'code', language: 'python' },
  js:        { kind: 'code', language: 'javascript' },
  mjs:       { kind: 'code', language: 'javascript' },
  cjs:       { kind: 'code', language: 'javascript' },
  ts:        { kind: 'code', language: 'typescript' },
  tsx:       { kind: 'code', language: 'typescript' },
  jsx:       { kind: 'code', language: 'javascript' },
  rs:        { kind: 'code', language: 'rust' },
  go:        { kind: 'code', language: 'go' },
  java:      { kind: 'code', language: 'java' },
  c:         { kind: 'code', language: 'c' },
  cpp:       { kind: 'code', language: 'cpp' },
  cc:        { kind: 'code', language: 'cpp' },
  h:         { kind: 'code', language: 'c' },
  hpp:       { kind: 'code', language: 'cpp' },
  rb:        { kind: 'code', language: 'ruby' },
  swift:     { kind: 'code', language: 'swift' },
  kt:        { kind: 'code', language: 'kotlin' },
  php:       { kind: 'code', language: 'php' },
  cs:        { kind: 'code', language: 'csharp' },

  css:       { kind: 'code', language: 'css' },
  scss:      { kind: 'code', language: 'scss' },

  sql:       { kind: 'code', language: 'sql' },
}

const NAME_TABLE: Record<string, FileClass> = {
  dockerfile: { kind: 'code', language: 'dockerfile' },
  makefile:   { kind: 'code', language: 'makefile' },
  rakefile:   { kind: 'code', language: 'ruby' },
  gemfile:    { kind: 'code', language: 'ruby' },
}

export function classifyPath(path: string): FileClass | null {
  const base = basename(path).toLowerCase()
  if (NAME_TABLE[base]) return { ...NAME_TABLE[base] }
  const ext = base.includes('.') ? base.split('.').pop()! : ''
  if (ext && EXT_TABLE[ext]) return { ...EXT_TABLE[ext] }
  return null
}

export function isSupportedPath(path: string): boolean {
  return classifyPath(path) !== null
}

/**
 * Returns the key under which a file's preferred edit mode is remembered.
 * Files with extensions key by the extension only (`foo.md` → `md`); files
 * without extensions or dotfile names key by the full lowercased basename
 * (`Dockerfile` → `dockerfile`, `.env` → `.env`). Always lowercased.
 */
export function modeKeyFor(path: string): string {
  const base = basename(path).toLowerCase()
  const dot = base.lastIndexOf('.')
  if (dot <= 0) return base
  return base.slice(dot + 1)
}

/**
 * Heuristic: does the content look like a binary file?
 * Returns true if the first 8KB contains a NUL byte, or
 * more than 5% of bytes are control characters outside whitespace.
 *
 * Limitation: UTF-16 / UTF-32 with BOM will look binary because of NUL bytes.
 * Acceptable for v1.
 */
export function looksBinary(s: string): boolean {
  const sample = s.slice(0, 8192)
  if (sample.indexOf('\x00') >= 0) return true
  if (sample.length === 0) return false
  let nonText = 0
  for (let i = 0; i < sample.length; i++) {
    const c = sample.charCodeAt(i)
    // Allow TAB(9), LF(10), VT(11), FF(12), CR(13); reject other control chars
    if (c < 9 || (c > 13 && c < 32)) nonText++
  }
  return nonText / sample.length > 0.05
}

export interface FileStat {
  /** Last-modification time in milliseconds since epoch. */
  mtime: number
  /** File size in bytes. */
  size: number
}

/**
 * Stat a path. Returns null if the file does not exist or stat throws.
 * Used by external-change detection to compare the on-disk state against
 * what we last accepted.
 */
export async function statFile(path: string): Promise<FileStat | null> {
  try {
    const info = await fsStat(path)
    return {
      mtime: info.mtime ? info.mtime.getTime() : 0,
      size: info.size ?? 0,
    }
  } catch {
    return null
  }
}
