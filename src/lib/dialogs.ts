import { message, save as saveDialog, open as openDialog } from '@tauri-apps/plugin-dialog'
import { t } from './i18n/store.svelte'
import { pushToast } from './toast.svelte'
import type { DirtyChoice } from './tabs.svelte'
import { basename } from './fs'

const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'heic', 'heif', 'avif']

const SUBTITLE_EXTS = ['srt', 'vtt', 'ass', 'ssa']

const ALL_EXTS = [
  'md', 'markdown', 'mdown', 'mkd', 'mdx',
  'html', 'htm',
  'txt', 'text', 'log', 'csv', 'tsv', 'env',
  ...SUBTITLE_EXTS,
  'rst', 'org', 'adoc', 'asciidoc', 'tex',
  'diff', 'patch', 'properties',
  'json', 'jsonc', 'yaml', 'yml', 'toml', 'ini', 'conf', 'xml',
  'sh', 'bash', 'zsh',
  'py', 'js', 'mjs', 'cjs', 'ts', 'tsx', 'jsx',
  'rs', 'go', 'java', 'c', 'cpp', 'cc', 'h', 'hpp',
  'rb', 'swift', 'kt', 'php', 'cs',
  'css', 'scss', 'sql',
  ...IMAGE_EXTS,
]

/**
 * Confirm-before-close for NAMED dirty files — a single macOS-standard
 * three-button alert (Save / Don't Save / Cancel).
 * Untitled dirty files are handled directly in closeTab (NSSavePanel).
 *
 * `title` renders as NSAlert's bold headline (carries the filename);
 * the first arg renders as the gray informative text.
 */
export async function confirmDirtyClose(name: string): Promise<DirtyChoice> {
  // NOTE: message() with custom buttons returns the clicked button's LABEL text
  // (not 'Yes'/'No'/'Cancel'), so we compare against the exact labels we pass.
  const saveLabel = t('dialog.save')
  const dontSaveLabel = t('dialog.dontSave')
  const cancelLabel = t('common.cancel')
  const res = await message(t('dialog.saveChanges.info'), {
    title: t('dialog.saveChanges.message', { name }),
    kind: 'warning',
    buttons: { yes: saveLabel, no: dontSaveLabel, cancel: cancelLabel },
  })
  if (res === saveLabel) return 'save'
  if (res === dontSaveLabel) return 'discard'
  return 'cancel'   // Cancel button, Esc, or window dismiss → keep editing
}

export async function pickOpenFile(): Promise<string | null> {
  const picked = await openDialog({
    multiple: false,
    filters: [
      { name: 'Markdown', extensions: ['md', 'markdown', 'mdown', 'mkd', 'mdx'] },
      { name: 'HTML', extensions: ['html', 'htm'] },
      { name: 'Subtitles', extensions: SUBTITLE_EXTS },
      { name: 'Images', extensions: IMAGE_EXTS },
      { name: 'All supported', extensions: ALL_EXTS },
    ],
  })
  return typeof picked === 'string' ? picked : null
}

/** Friendly "File Format" filter names shown in the NSSavePanel dropdown. */
function saveFilters(ext?: string) {
  if (!ext) return [
    { name: 'Markdown', extensions: ['md', 'markdown', 'mdown', 'mkd', 'mdx'] },
    { name: 'All supported', extensions: ALL_EXTS },
  ]
  if (['md', 'markdown', 'mdown', 'mkd', 'mdx'].includes(ext))
    return [{ name: 'Markdown', extensions: ['md', 'markdown', 'mdown', 'mkd', 'mdx'] }]
  if (['html', 'htm'].includes(ext))
    return [{ name: 'HTML', extensions: ['html', 'htm'] }]
  if (['txt', 'text', 'log'].includes(ext))
    return [{ name: 'Plain Text', extensions: ['txt', 'text', 'log'] }]
  if (SUBTITLE_EXTS.includes(ext))
    return [{ name: 'Subtitles', extensions: SUBTITLE_EXTS }]
  if (IMAGE_EXTS.includes(ext))
    return [{ name: 'Image', extensions: IMAGE_EXTS }]
  if (ext === 'base')
    return [{ name: 'Base', extensions: ['base'] }]
  if (ALL_EXTS.includes(ext))
    return [{ name: ext.toUpperCase(), extensions: [ext] }]
  return [{ name: 'All supported', extensions: ALL_EXTS }]
}

/**
 * Open the native NSSavePanel.
 * - No defaultPath → resolves to Documents/untitled.md (avoids empty dir)
 * - With defaultPath → pre-fills filename and navigates to that directory
 */
export async function pickSaveFile(defaultPath?: string): Promise<string | null> {
  let resolvedPath = defaultPath
  if (!resolvedPath) {
    try {
      const { documentDir } = await import('@tauri-apps/api/path')
      resolvedPath = `${(await documentDir()).replace(/\/$/, '')}/untitled.md`
    } catch {
      resolvedPath = 'untitled.md'
    }
  }
  const ext = basename(resolvedPath).split('.').pop()?.toLowerCase()
  const picked = await saveDialog({ defaultPath: resolvedPath, filters: saveFilters(ext) })
  return picked ?? null
}

export function showError(text: string): void {
  pushToast({ level: 'error', message: text })
}
