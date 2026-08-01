export interface CliPayload {
  subcommand: string
  plugin_id: string
  plugin_command: string
  file: string | null
  /** Rust側 serde_json::Map,但 parse_subcommand_args 只写入 Bool(true) 与
   *  String 两种值(见 src-tauri/src/cli/runner.rs),故此窄类型是准确契约。 */
  flags: Record<string, string | boolean>
  global: GlobalFlags
}

export interface GlobalFlags {
  json: boolean
  quiet: boolean
  clipboard: boolean
  yes: boolean
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
