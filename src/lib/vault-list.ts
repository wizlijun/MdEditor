const MARKDOWN_EXTS = new Set(['md', 'markdown', 'mdown', 'mkd', 'mdx'])
const HTML_EXTS = new Set(['html', 'htm'])
const TEXT_EXTS = new Set(['txt', 'log', 'csv', 'tsv', 'env'])
const IMAGE_EXTS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'heic', 'heif', 'avif'])

export function fileIcon(ext: string): string {
  const e = ext.toLowerCase()
  if (MARKDOWN_EXTS.has(e)) return '📝'
  if (HTML_EXTS.has(e)) return '🌐'
  if (IMAGE_EXTS.has(e)) return '🖼️'
  if (TEXT_EXTS.has(e)) return '📄'
  return '📄'
}

export function isImage(ext: string): boolean {
  return IMAGE_EXTS.has(ext.toLowerCase())
}

export function isText(ext: string): boolean {
  const e = ext.toLowerCase()
  return MARKDOWN_EXTS.has(e) || HTML_EXTS.has(e) || TEXT_EXTS.has(e)
}

export interface VaultListEntry {
  name: string
  kind: 'file' | 'dir'
  size: number | null
  mtime: number | null
  ext: string | null
}
