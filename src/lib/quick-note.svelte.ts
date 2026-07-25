// Quick-note: create a timestamped markdown file in the vault's inbox and open
// it for editing. Triggered from the tray "New Markdown" item and the
// system-wide Cmd+Ctrl+M hotkey (both emit the `quick-note` event, wired in
// App.svelte). The inbox sub-directory is a vault-scoped setting
// (`{vault}/.notemd/settings.json`, key `inboxDir`, default `inbox`).

import { invoke } from '@tauri-apps/api/core'
import { mkdir, exists } from '@tauri-apps/plugin-fs'
import { openFile, openPathBackedMarkdownDraft } from './tabs.svelte'
import { requestEditorFocus } from './editor-focus.svelte'
import { pushToast } from './toast.svelte'
import { t } from './i18n/store.svelte'
import { quickNoteFileName } from './quick-note-name'

export { quickNoteFileName }

export const DEFAULT_INBOX_DIR = 'inbox'

/** The configured inbox directory name (vault-relative). Surfaced in Settings;
 *  the actual quick-note path is resolved backend-side via `notemd_quick_note_dir`. */
export const inboxDir = $state<{ value: string }>({ value: DEFAULT_INBOX_DIR })

interface VaultSettingsDto {
  inboxDir?: string | null
}

/** Load the inbox dir from vault settings (call alongside loadOutlineDirs). */
export async function loadInboxDir(): Promise<void> {
  const dto = await invoke<VaultSettingsDto>('notemd_vault_settings_get').catch(
    () => ({}) as VaultSettingsDto,
  )
  const v = dto?.inboxDir
  inboxDir.value = typeof v === 'string' && v.trim() !== '' ? v : DEFAULT_INBOX_DIR
}

/** Persist a new inbox dir name; empty/whitespace falls back to the default. */
export async function setInboxDir(raw: string): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    inboxDir: raw.trim() || DEFAULT_INBOX_DIR,
  })
  inboxDir.value = merged?.inboxDir || DEFAULT_INBOX_DIR
}

/**
 * Open a path-backed quick note for "now", focusing the editor in edit state.
 * The file is created lazily on first non-empty save/autosave, so dismissing an
 * untouched quick note never leaves a 0-byte file. Once the note grows an H1
 * title, the first save renames it after that title (see `quick-note-name`).
 */
export async function createQuickNote(now: Date = new Date()): Promise<void> {
  let dir: string
  try {
    dir = await invoke<string>('notemd_quick_note_dir')
  } catch {
    pushToast({ level: 'warn', message: t('quickNote.noVault') })
    return
  }
  const fullPath = `${dir.replace(/\/+$/, '')}/${quickNoteFileName(now)}`
  try {
    await mkdir(dir, { recursive: true })
    // Set the focus request BEFORE openFile so the editor consumes it on mount.
    requestEditorFocus(fullPath)
    if (await exists(fullPath).catch(() => false)) {
      await openFile(fullPath)
    } else {
      // No explicit mode: the draft opens in the editor's remembered mode for
      // `.md`, the same one openFile uses and the mode toggle persists.
      await openPathBackedMarkdownDraft(fullPath, '', { skipEmptySave: true })
    }
  } catch (e) {
    pushToast({ level: 'error', message: t('quickNote.createFailed'), detail: String(e) })
  }
}
