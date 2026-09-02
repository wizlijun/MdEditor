export interface CliPayload {
  subcommand: string
  plugin_id: string
  plugin_command: string
  /** Manifest-declared positional arguments, keyed by their declared names. */
  args: Record<string, string | number>
  /** Rust側 serde_json::Map,但 parse_subcommand_args 只写入 Bool(true) 与
   *  String 两种值(见 src-tauri/src/cli/runner.rs),故此窄类型是准确契约。 */
  flags: Record<string, string | boolean>
  global: GlobalFlags
}

export interface GlobalFlags {
  json: boolean
  quiet: boolean
  clipboard: boolean
}

/**
 * Does this CLI entry actually need a file argument? A subcommand that
 * declares no required `path` arg (e.g. `notemd roam-day --date …`) must not
 * be rejected for missing one.
 */
export function requiresFileArg(
  entry: { args?: Array<{ type?: string; required?: boolean }> } | undefined,
): boolean {
  return (entry?.args ?? []).some((a) => a.type === 'path' && a.required === true)
}

/** First manifest-declared path positional, used as the virtual-tab source. */
export function firstPathArg(
  entry: { args?: Array<{ name: string; type?: string }> } | undefined,
  args: CliPayload['args'],
): string | undefined {
  const name = (entry?.args ?? []).find((a) => a.type === 'path')?.name
  const value = name ? args[name] : undefined
  return typeof value === 'string' ? value : undefined
}

export function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || path.startsWith('\\\\') || /^[A-Za-z]:[\\/]/.test(path)
}

export function dirnameOf(path: string): string {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  if (slash < 0) return ''
  if (slash === 0) return path.slice(0, 1)
  // Preserve a Windows drive root (`C:\\file.md` → `C:\\`).
  if (slash === 2 && /^[A-Za-z]:/.test(path)) return path.slice(0, 3)
  return path.slice(0, slash)
}

export function joinPath(parent: string, child: string): string {
  if (!parent) return child
  const separator = parent.includes('\\') && !parent.includes('/') ? '\\' : '/'
  const normalizedChild = child.replace(/[\\/]+/g, separator)
  return `${parent.replace(/[\\/]+$/, '')}${separator}${normalizedChild}`
}

export function replaceExtension(path: string, extension: string): string {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  const dot = path.lastIndexOf('.')
  return `${dot > slash ? path.slice(0, dot) : path}${extension}`
}

/** Resolve an output flag relative to the input file on both Unix and Windows. */
export function outputPathFor(inputPath: string, outputFlag?: string): string {
  if (!outputFlag) return replaceExtension(inputPath, '.pdf')
  return isAbsolutePath(outputFlag) ? outputFlag : joinPath(dirnameOf(inputPath), outputFlag)
}

/** Extract filename from absolute path. */
export function basenameOf(absPath: string): string {
  const slash = Math.max(absPath.lastIndexOf('/'), absPath.lastIndexOf('\\'))
  return slash >= 0 ? absPath.slice(slash + 1) : absPath
}

/** Extract extension (with dot) or null. */
export function extensionOf(filename: string): string | null {
  const dot = filename.lastIndexOf('.')
  return dot > 0 ? filename.slice(dot) : null
}

/**
 * Determine the kind of a CLI virtual tab. Mirrors src/lib/fs.ts classifyPath
 * but kept inline because we don't want to load full editor state. The
 * 'plaintext' bucket is purely a CLI-side label — the Svelte runner maps it
 * to the editor's `FileKind` (which has no 'plaintext'; we fold to 'code')
 * before constructing the virtual Tab.
 */
export function inferKind(extension: string | null): 'markdown' | 'html' | 'code' | 'plaintext' | 'image' {
  if (extension == null) return 'plaintext'
  const e = extension.toLowerCase()
  if (e === '.md' || e === '.markdown' || e === '.mdown' || e === '.mkd') return 'markdown'
  if (e === '.html' || e === '.htm') return 'html'
  if (['.png', '.jpg', '.jpeg', '.gif', '.svg', '.webp', '.avif', '.heic', '.heif'].includes(e)) return 'image'
  return 'code'
}
