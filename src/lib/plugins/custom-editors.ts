/**
 * Custom-editor registry (子项目④).
 *
 * Scans loaded plugin manifests for `contributes.custom_editors` and builds a
 * `file extension → editor` map so `openFile` can route a `.base` (or any
 * claimed extension) to a plugin-served iframe editor instead of a built-in
 * view. The extension is stored WITHOUT a leading dot and lowercased, matching
 * how `classifyPath` derives extensions.
 *
 * v1 plugins never carry `custom_editors`, so a v1-only host yields an empty
 * map and the whole feature stays inert.
 */

import type { PluginManifest } from './types'

/** What a registered custom editor needs to render its iframe. */
export interface CustomEditorRef {
  pluginId: string
  editorId: string
  /** UI-relative entry path served under `plugin://<pluginId>/`. */
  entry: string
}

/** Normalise an extension token to bare-lowercase (`'.Base'` → `'base'`). */
function normExt(ext: string): string {
  return ext.replace(/^\.+/, '').toLowerCase()
}

/**
 * Extensions a plugin may NEVER claim as a custom editor. Guards the core
 * document types against a (future third-party) plugin hijacking every
 * markdown/text file into its own iframe. `note.md`/`notes.md` are compound
 * suffixes; `classifyPath` keys on the last dot segment (`md`), so guarding
 * `md` covers them. Kept minimal — extend if new core types appear.
 */
const RESERVED_EXTENSIONS = new Set([
  'md', 'markdown', 'mdown', 'mkd', 'txt', 'html', 'htm', 'canvas',
])

/**
 * Build a `Map<ext, CustomEditorRef>` from every plugin's `custom_editors`.
 *
 * Extensions are keyed bare + lowercased. When two plugins claim the same
 * extension the FIRST one wins (manifests are iterated in array order); a
 * warning is logged so the collision is visible. A plugin with no
 * `custom_editors` contributes nothing.
 */
export function buildCustomEditorRegistry(
  manifests: PluginManifest[],
): Map<string, CustomEditorRef> {
  const map = new Map<string, CustomEditorRef>()
  for (const m of manifests) {
    const editors = m.custom_editors
    if (!editors || editors.length === 0) continue
    for (const ed of editors) {
      if (!ed || !ed.id || !ed.entry || !Array.isArray(ed.file_extensions)) continue
      const ref: CustomEditorRef = { pluginId: m.id, editorId: ed.id, entry: ed.entry }
      for (const raw of ed.file_extensions) {
        const ext = normExt(raw)
        if (!ext) continue
        if (RESERVED_EXTENSIONS.has(ext)) {
          console.warn(
            `[custom-editors] plugin '${m.id}' may not claim reserved core ` +
              `extension '.${ext}'; ignoring`,
          )
          continue
        }
        if (map.has(ext)) {
          console.warn(
            `[custom-editors] extension '.${ext}' claimed by both ` +
              `'${map.get(ext)!.pluginId}' and '${m.id}'; keeping the first`,
          )
          continue
        }
        map.set(ext, ref)
      }
    }
  }
  return map
}

/**
 * Look up the custom editor registered for `ext` (with or without a leading
 * dot) across `manifests`, or `null` when none is registered. Convenience
 * wrapper over `buildCustomEditorRegistry` for one-off lookups.
 */
export function customEditorFor(
  ext: string,
  manifests: PluginManifest[],
): CustomEditorRef | null {
  return buildCustomEditorRegistry(manifests).get(normExt(ext)) ?? null
}
