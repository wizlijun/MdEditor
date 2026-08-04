// editor-kit.ts — loads the host's Editor Kit into this plugin window.
//
// The kit is NOT bundled with the plugin: it's the host's own rich/source
// markdown editor, built as a second entry of the main frontend and served to
// `editor.kit`-capable plugin windows over the `plugin://` protocol's reserved
// `__host__/` path (src-tauri/src/plugin_runtime/protocol.rs). That mapping is
// a read-only mirror of the host's `dist/assets/` tree, hence the `assets/`
// segment in the URL below — dropping it 404s.
//
// The type declarations mirror `src/editor-kit/main.ts`'s frozen v1 API. They
// are hand-copied because there is no build-time path from a plugin package to
// the host's source; if the host ever ships a v2 entry, this file (and the
// `-v1` in the URL) is the single place that changes.
import { bridge } from './bridge'

export type KitMode = 'rich' | 'source'

export interface KitEditor {
  getMarkdown(): string
  setMarkdown(md: string): void
  getMode(): KitMode
  /** Flushes a pending `onChange` before switching panes. */
  setMode(m: KitMode): Promise<void>
  /**
   * Replaces the empty-buffer hint in the live pane and for every later mode
   * switch. `KitOptions.placeholder` is read once at mount, so this is the only
   * way the rotating prompt on a new idea actually changes what the user sees.
   */
  setPlaceholder(text: string): void
  focus(): void
  /** Flushes a pending `onChange` before tearing down. */
  destroy(): void
}

export interface KitOptions {
  initialMarkdown: string
  mode?: KitMode
  /** Debounced 200 ms in rich mode; immediate in source mode. */
  onChange?: (md: string) => void
  placeholder?: string
  /** Vault-relative dir of the edited document, for resolving relative images. */
  baseDir?: string
}

export type MountMarkdownEditor = (container: HTMLElement, opts: KitOptions) => Promise<KitEditor>

/** Where the host serves the kit for *this* plugin id. */
export function kitUrl(): string {
  return `plugin://${bridge().pluginId}/__host__/assets/editor-kit-v1.js`
}

/**
 * Dynamically imports the kit and returns its mount function.
 *
 * Throws when the host is too old (no `__host__` route → 404), when the plugin
 * lacks the `editor.kit` capability (also 404), or when the module doesn't
 * export `mountMarkdownEditor`. Callers must degrade to a plain textarea rather
 * than leave the window blank — see App.svelte.
 */
export async function loadKit(): Promise<MountMarkdownEditor> {
  const mod = (await import(/* @vite-ignore */ kitUrl())) as { mountMarkdownEditor?: unknown }
  const mount = mod.mountMarkdownEditor
  if (typeof mount !== 'function') {
    throw new Error('editor-kit-v1.js loaded but exports no mountMarkdownEditor')
  }
  return mount as MountMarkdownEditor
}
