import { basename, dirname } from '../paths'

/**
 * Render a `default_filename` template against the active tab's path.
 * Supported placeholders:
 *   {basename}  - filename including extension (e.g. "foo.md")
 *   {stem}      - basename minus the last extension; dotfiles keep their full name
 *   {ext}       - the last extension (no dot); empty for files with no extension
 *   {dir}       - parent directory (no trailing slash); "/" for root paths
 *
 * Unknown placeholders are kept verbatim (no errors thrown). When `filePath`
 * is null or empty, all placeholders fall back to a synthetic "untitled"
 * path so the user still sees a sensible default in the save dialog.
 */
export function renderFilenameTemplate(template: string, filePath: string | null): string {
  const path = filePath && filePath.length > 0 ? filePath : '/untitled'

  const base = basename(path)
  const dot = base.lastIndexOf('.')
  // dot <= 0 catches no-extension and dotfile cases (".env" → stem ".env")
  const stem = dot <= 0 ? base : base.slice(0, dot)
  const ext  = dot <= 0 ? '' : base.slice(dot + 1)
  const dir  = dirname(path)

  return template.replace(/\{(basename|stem|ext|dir)\}/g, (_, name: string) => {
    switch (name) {
      case 'basename': return base
      case 'stem':     return stem
      case 'ext':      return ext
      case 'dir':      return dir
      default:         return `{${name}}`
    }
  })
}
