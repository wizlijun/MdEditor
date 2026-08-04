// Editor Kit — the rich/source markdown editor the host hands to isolated
// plugin webviews at runtime (spec §3.4).
//
// Built as a second vite entry of the main frontend, so it shares the moraya /
// prosemirror chunks the main window already ships (installer growth ≈ 0) and
// can never drift from the main editor's styling or highlighting. Plugins load
// it with `await import('plugin://<id>/__host__/assets/editor-kit-v1.js')` and
// call `mountMarkdownEditor()`. The `assets/` segment is NOT optional: the
// protocol handler maps `__host__/<rel>` onto the host dist tree and only
// `dist/assets/` is reachable, so dropping it 404s.
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
  /** Switches panes. Flushes any pending `onChange` first (see below). */
  setMode(m: KitMode): Promise<void>
  focus(): void
  /** Tears the editor down. Flushes any pending `onChange` first (see below). */
  destroy(): void
}

export interface KitOptions {
  /**
   * NOTE ON THE CONTAINER (see `mountMarkdownEditor`): it must have a
   * determinate height. The kit lays itself out with `height: 100%` and
   * absolute positioning, so a container that sizes to its content collapses
   * source mode to zero height.
   */
  initialMarkdown: string
  /** Default 'rich'. */
  mode?: KitMode
  /**
   * Debounced by the editor itself (200 ms in rich mode).
   *
   * `setMode()` and `destroy()` flush a pending change synchronously before
   * they do anything else, so a consumer that persists purely on `onChange`
   * never loses the last edits to a mode switch or a closing window.
   */
  onChange?: (md: string) => void
  /** Hint shown in an empty buffer, in either rich or source mode. */
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

/**
 * Mount the editor into `container` (v1 API — frozen).
 *
 * **`container` MUST have a determinate height** (a flex/grid child with
 * `min-height: 0`, an explicit `height`, or absolute insets). The kit fills its
 * container with `height: 100%` plus absolutely-positioned source-mode layers,
 * so it contributes no intrinsic height of its own: drop it into a
 * content-sized box and rich mode looks fine while source mode collapses to
 * zero height and appears blank.
 */
export async function mountMarkdownEditor(container: HTMLElement, opts: KitOptions): Promise<KitEditor> {
  injectKitCss()
  await applyKitTheme()
  watchKitTheme()

  // Unconditional: moraya's document base dir is module-global state, so a
  // second mount that omits `baseDir` would silently inherit the previous
  // mount's directory and resolve relative images against the wrong folder.
  const root = await vaultRoot()
  setKitBaseDir(root ? joinAbsolute(root, opts.baseDir ?? '') : '')

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
    if (mode === 'rich') rich = await mountRich(host, markdown, root, emit, opts.placeholder)
    else source = mountSource(host, markdown, emit, opts.placeholder)
  }

  await mountCurrent()

  // Markdown is the single source of truth: reading it always goes to whichever
  // pane is live, and switching modes carries the live text across.
  const currentMarkdown = () =>
    (mode === 'rich' ? rich?.getMarkdown() : source?.getValue()) ?? markdown

  /**
   * Emit anything the live pane holds but has not reported yet.
   *
   * Rich mode debounces `onChange` by 200 ms and moraya's change plugin only
   * *clears* that timer on destroy (it does not flush it — see
   * `moraya-core/src/setup.ts` `destroy()`), so tearing the editor down inside
   * the debounce window would drop the user's last keystrokes on the floor:
   * the text survives inside the kit but the consumer — which persists on
   * `onChange` — never hears about it.
   */
  const flush = () => {
    const cur = currentMarkdown()
    if (cur === markdown) return
    markdown = cur
    opts.onChange?.(cur)
  }

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
      flush()
      mode = m
      await mountCurrent()
    },
    focus: () => { if (mode === 'rich') rich?.view.focus(); else source?.focus() },
    destroy: () => {
      flush()
      rich?.destroy(); rich = null
      source?.destroy(); source = null
      host.remove()
    },
  }
}
