// Rich mode for the Editor Kit: a @moraya/core editor with the Tauri-backed
// pieces (renderer registry, link opener, spreadsheet / frontmatter views,
// insights analytics plugin) left out — a plugin webview has no Tauri IPC.
//
// The option values below mirror `mountRichEditor` in `src/lib/editor-bridge.ts`
// (that file cannot be imported here: it drags in tabs / insights / Tauri
// adapters). Keep the two in sync when either changes.

import { createEditor, createEditorPlugins, parseMarkdown, setDocumentBaseDir, type CreateEditorOptions, type MorayaEditorInstance } from '@moraya/core'
import { bridgeMediaResolver } from './media'
// placeholder-plugin only depends on prosemirror-state/view — zero Tauri IPC,
// so it clears the kit's dependency allowlist.
import { placeholderPlugin } from '../lib/placeholder-plugin'
import { isApplePlatformSync } from '../lib/platform-sync'
import { powerModePlugin, type ConfigGetter } from '../lib/power-mode/plugin'
import { createImeGuard, guardRichEditor, type ImeGuard } from '../lib/ime'
import { handleSelectAllKeydown, type SelectAllTarget } from '../lib/editor-select-all'
import { EditorState, type Plugin } from 'prosemirror-state'
import type { Schema } from 'prosemirror-model'

/** Base directory (absolute) used to resolve relative image paths in rich mode. */
export function setKitBaseDir(absoluteDir: string): void {
  setDocumentBaseDir(absoluteDir)
}

/**
 * The placeholder plugin only when a hint was given. Pulled out as a pure
 * function so the wiring can be checked in a DOM-less test (mounting a real
 * ProseMirror + moraya editor does not work under jsdom).
 */
export function richPlugins(placeholder: string | undefined): Plugin[] {
  return placeholder ? [placeholderPlugin(placeholder)] : []
}

/**
 * The plugin list `plugins` would become if the placeholder said `text`.
 *
 * `placeholderPlugin` bakes its text in at construction time (it is closed over
 * by the `decorations` prop), so changing the hint means building a NEW plugin
 * and swapping it into the editor's configuration — there is no setter.
 *
 * Only the placeholder is touched: the outgoing one is matched by ProseMirror
 * plugin key (all `placeholderPlugin` instances share one module-level
 * `PluginKey`, so this is exact and can never catch one of moraya's own
 * plugins), everything else is carried over in order. Dropping moraya's
 * plugins here would take the editor's history, input rules and keymaps with
 * them. Mounting without a placeholder and setting one later works too — the
 * filter simply matches nothing and the new plugin is appended.
 *
 * Pure, and separate from `setRichPlaceholder`, so the swap can be asserted in
 * a DOM-less test (an `EditorView` needs a real layout engine).
 */
export function swapPlaceholder(plugins: readonly Plugin[], text: string): Plugin[] {
  const next = placeholderPlugin(text)
  const key = (next as unknown as { key: string }).key
  return plugins.filter((p) => (p as unknown as { key: string }).key !== key).concat(next)
}

/** Applies {@link swapPlaceholder} to a live editor. */
export function setRichPlaceholder(instance: MorayaEditorInstance, text: string): void {
  const { view } = instance
  view.updateState(view.state.reconfigure({ plugins: swapPlaceholder(view.state.plugins, text) }))
}

/**
 * Capture Select All on the kit host, before moraya's own `Mod-a` keymap can
 * narrow a code-block selection. The listener shares the pane's IME guard and
 * follows the editor instance's lifecycle so mode switches leave nothing
 * behind on the reusable host element.
 */
function guardRichSelectAll<T extends { view: SelectAllTarget; destroy(): void }>(
  host: HTMLElement,
  instance: T,
  ime: ImeGuard,
): T {
  const onKeydown = (event: Event) => {
    const keyEvent = event as KeyboardEvent
    if (ime.blocks(keyEvent)) return
    handleSelectAllKeydown(keyEvent, instance.view)
  }
  host.addEventListener('keydown', onKeydown, true)

  const destroy = instance.destroy.bind(instance)
  let active = true
  instance.destroy = () => {
    if (!active) return
    active = false
    host.removeEventListener('keydown', onKeydown, true)
    destroy()
  }
  return instance
}

export async function mountRich(
  host: HTMLElement,
  initial: string,
  vaultRoot: string,
  onChange: (md: string) => void,
  placeholder?: string,
  getPowerMode?: ConfigGetter,
  customizeSchema?: (schema: Schema) => Schema,
): Promise<MorayaEditorInstance> {
  const options: CreateEditorOptions = {
    container: host,
    initialContent: initial,
    mediaResolver: bridgeMediaResolver(vaultRoot),
    platform: { getCurrentFilePath: () => null, isMacOS: isApplePlatformSync() },
    // Math / mermaid pull heavy renderers the kit's consumers do not need
    // (spec §3.4: kit options are narrowed for plugin windows).
    enableMath: false,
    enableMermaid: false,
    enableTableResize: true,
    enableImageSelection: false,
    enableHistory: true,
    // Do NOT auto-format inline markers as you type: `**`, `__`, `*`, `_`,
    // `` ` ``, `~~`, `^^`, `==` stay literal instead of collapsing into a mark.
    enableInlineMarkInputRules: false,
    // Marks parsed from the file still render; on the caret's line their source
    // delimiters are revealed (Live-Preview style) and re-render on exit.
    inlineSyntaxScope: 'line',
    onChange,
    changeDebounceMs: 200,
  }
  const instance = await createEditor(options)
  // The v2 surface needs internal node identity attributes that participate in
  // native history. Rebuild schema-bound plugins once at mount; retain Moraya's
  // NodeViews and lifecycle. v1 never takes this branch or changes its schema.
  if (customizeSchema) {
    const schema = customizeSchema(instance.view.state.schema)
    const plugins = (await createEditorPlugins(options, schema)).filter((plugin) => {
      const key = (plugin as unknown as { key: string }).key
      // Governed content cannot treat focusing a link as an edit. These two
      // live-preview plugins expand marks into literal source / insert cursor
      // sentinel text. v2 uses real marks, decorations and explicit commands;
      // v1 retains Moraya's original live-preview behavior unchanged.
      return !key.startsWith('moraya-link-text$') && !key.startsWith('moraya-inline-code-convert$')
    })
    instance.view.updateState(EditorState.create({
      schema,
      doc: parseMarkdown(initial, schema),
      plugins,
    }))
  }

  // Append the placeholder plugin after mount, same construction as the main
  // window's editor-append plugins in RichEditor.svelte (`view.updateState(
  // view.state.reconfigure(...))`).
  //
  // Power Mode 也在这里接:getter 每次击键现取,所以 setPowerMode() 换配置不需要
  // 重挂编辑器(重挂会丢光标、选区和撤销栈)。
  const extra: Plugin[] = richPlugins(placeholder)
  if (getPowerMode) extra.push(powerModePlugin(getPowerMode, () => 'kit'))
  if (extra.length) {
    instance.view.updateState(
      instance.view.state.reconfigure({
        plugins: instance.view.state.plugins.concat(extra),
      }),
    )
  }
  const ime = createImeGuard()
  return guardRichSelectAll(host, guardRichEditor(host, instance, ime), ime)
}
