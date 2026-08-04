// Editor Kit — the rich/source markdown editor the host hands to isolated
// plugin webviews at runtime (spec §3.4).
//
// Built as a second vite entry of the main frontend, so it shares the moraya /
// prosemirror chunks the main window already ships (installer growth ≈ 0) and
// can never drift from the main editor's styling or highlighting. Plugins load
// it with `await import('plugin://<id>/__host__/editor-kit-v1.js')` and call
// `mountMarkdownEditor()`.
//
// Hard constraint: nothing in this module's dependency graph may touch Tauri
// IPC (`@tauri-apps/*`, `src/lib/editor-bridge.ts`, tabs, insights, adapters).
// A plugin webview has no IPC — everything host-side goes through
// `window.notemd`.

// kit.css `@import`s ../styles/editor-base.css, so the one emitted stylesheet
// carries the shared editor skin as well (see the note at the top of kit.css).
import './kit.css'
import { mountRich, setKitBaseDir } from './rich'
import { mountSource, type SourcePane } from './source'
import { loadVaultRoot } from './media'
import { applyKitTheme, watchKitTheme } from './theme'

export type KitMode = 'rich' | 'source'

/** v1 API — frozen. Breaking changes ship as `editor-kit-v2.js`. */
export interface KitEditor {
  getMarkdown(): string
  setMarkdown(md: string): void
  getMode(): KitMode
  setMode(m: KitMode): Promise<void>
  focus(): void
  destroy(): void
}

export interface KitOptions {
  initialMarkdown: string
  /** Default 'rich'. */
  mode?: KitMode
  /** Debounced by the editor itself (200 ms in rich mode). */
  onChange?: (md: string) => void
  /** Hint shown in an empty source-mode buffer. */
  placeholder?: string
  /**
   * Vault-relative directory of the document being edited, used to resolve
   * relative image paths. Omit when the content has no local images.
   */
  baseDir?: string
}

/** The vault root is stable for the window's lifetime; ask the host once. */
let vaultRootPromise: Promise<string> | null = null
function vaultRoot(): Promise<string> {
  vaultRootPromise ??= loadVaultRoot()
  return vaultRootPromise
}

/**
 * The entry's stylesheet is emitted next to it (`editor-kit-v1.css`) and is not
 * auto-injected for a JS entry, so pull it in relative to this module's own URL
 * — which resolves under `plugin://<id>/__host__/` in a plugin window.
 */
function injectKitCss(): void {
  // @vite-ignore — the stylesheet is a sibling build artifact, not a source
  // asset vite can resolve; the URL must stay literal and resolve at runtime.
  const href = new URL(/* @vite-ignore */ './editor-kit-v1.css', import.meta.url).href
  if (document.querySelector(`link[href="${href}"]`)) return
  const link = document.createElement('link')
  link.rel = 'stylesheet'
  link.href = href
  document.head.appendChild(link)
}

function joinAbsolute(root: string, relDir: string): string {
  const base = root.endsWith('/') ? root.slice(0, -1) : root
  const rel = relDir.replace(/^\/+|\/+$/g, '')
  return rel ? `${base}/${rel}` : base
}

export async function mountMarkdownEditor(container: HTMLElement, opts: KitOptions): Promise<KitEditor> {
  injectKitCss()
  await applyKitTheme()
  watchKitTheme()

  const root = await vaultRoot()
  if (opts.baseDir !== undefined && root) setKitBaseDir(joinAbsolute(root, opts.baseDir))

  let markdown = opts.initialMarkdown
  let mode: KitMode = opts.mode ?? 'rich'

  const host = document.createElement('div')
  host.className = 'kit-host'
  container.appendChild(host)

  let rich: Awaited<ReturnType<typeof mountRich>> | null = null
  let source: SourcePane | null = null

  const emit = (md: string) => { markdown = md; opts.onChange?.(md) }

  async function mountCurrent(): Promise<void> {
    rich?.destroy(); rich = null
    source?.destroy(); source = null
    host.innerHTML = ''
    if (mode === 'rich') rich = await mountRich(host, markdown, root, emit)
    else source = mountSource(host, markdown, emit, opts.placeholder)
  }

  await mountCurrent()

  // Markdown is the single source of truth: reading it always goes to whichever
  // pane is live, and switching modes carries the live text across.
  const currentMarkdown = () =>
    (mode === 'rich' ? rich?.getMarkdown() : source?.getValue()) ?? markdown

  return {
    getMarkdown: currentMarkdown,
    setMarkdown: (md) => {
      markdown = md
      if (mode === 'rich') rich?.setContent(md)
      else source?.setValue(md)
    },
    getMode: () => mode,
    setMode: async (m) => {
      if (m === mode) return
      markdown = currentMarkdown()
      mode = m
      await mountCurrent()
    },
    focus: () => { if (mode === 'rich') rich?.view.focus(); else source?.focus() },
    destroy: () => {
      rich?.destroy(); rich = null
      source?.destroy(); source = null
      host.remove()
    },
  }
}
