import { appendExcerpt, appendExcerptToTree } from './excerpt'
import { planNoteHome } from './note-home'
import { companionPathFor, markDirty, outline } from './store.svelte'

export type ExcerptOutcome =
  | { ok: true; notePath: string }
  | { ok: false; reason: 'empty' | 'no-companion' | 'configure-vault' | 'failed' }

/**
 * Write a selection from a document into its sidecar note.
 *
 * For mdx this is the ONLY path from "I read something worth keeping" to a
 * durable file: the document is rendered read-only and never written, so the
 * excerpt itself carries the anchor. Note homing reuses `planNoteHome`, the
 * same rule used when writing a note against any file outside the vault.
 */
export async function excerptToNote(mainPath: string, selection: string): Promise<ExcerptOutcome> {
  if (!selection.trim()) return { ok: false, reason: 'empty' }
  const companion = companionPathFor(mainPath)
  if (companion == null) return { ok: false, reason: 'no-companion' }
  try {
    const fs = await import('@tauri-apps/plugin-fs')
    const { sotvaultStore, syncSourceToVaultAsHome, refreshSotvault } = await import('../sotvault.svelte')
    const plan = planNoteHome(mainPath, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
      legacyNoteExists: await fs.exists(companion).catch(() => false),
    })

    let notePath: string | null = null
    if (plan.action === 'use') notePath = plan.notePath
    else if (plan.action === 'sync') {
      const rec = await syncSourceToVaultAsHome(mainPath)
      notePath = companionPathFor(rec.vault_path)
      await refreshSotvault()
    } else return { ok: false, reason: 'configure-vault' }
    if (!notePath) return { ok: false, reason: 'no-companion' }

    // The sidecar panel holds this file's text in memory while attached, and
    // its next save would clobber anything written underneath it.
    if (outline.docPath === notePath) {
      if (!appendExcerptToTree(outline.tree, selection)) return { ok: false, reason: 'empty' }
      markDirty()   // explicit user action = intent to persist (see intent-save)
      return { ok: true, notePath }
    }

    const diskText = (await fs.exists(notePath).catch(() => false))
      ? await fs.readTextFile(notePath).catch(() => null)
      : ''
    if (diskText == null) return { ok: false, reason: 'failed' }
    const out = appendExcerpt(diskText, selection)
    if (out == null) return { ok: false, reason: 'empty' }
    await fs.writeTextFile(notePath, out)
    return { ok: true, notePath }
  } catch (e) {
    console.warn('[excerpt] failed:', e)
    return { ok: false, reason: 'failed' }
  }
}
