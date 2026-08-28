import type { RendererRegistry, RendererPluginModule } from '@moraya/core'
import { ensureMermaidCssStyleSheetCompatibility } from '../mermaid-cssom'

type Loader = () => Promise<RendererPluginModule>

interface PluginEntry {
  version: string
  load: Loader
}

function escapeRenderError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e)
  return msg.replace(/[&<>"]/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c] as string),
  )
}

const dotPlugin: PluginEntry = {
  version: '1',
  load: async () => {
    const { instance } = await import('@viz-js/viz')
    let vizPromise: Promise<Awaited<ReturnType<typeof instance>>> | null = null
    const getViz = () => (vizPromise ??= instance())

    return {
      async render(source, container) {
        container.innerHTML = ''
        if (!source.trim()) {
          container.innerHTML = '<div class="renderer-empty">Empty graph</div>'
          return
        }
        const viz = await getViz()
        try {
          const svg = viz.renderSVGElement(source)
          container.appendChild(svg)
        } catch (e) {
          container.innerHTML = `<div class="renderer-error">${escapeRenderError(e)}</div>`
        }
      },
      destroy(container) {
        container.innerHTML = ''
      },
    }
  },
}

interface MermaidLike {
  initialize: (cfg: { startOnLoad?: boolean; securityLevel?: string }) => void
  render: (id: string, src: string) => Promise<{ svg: string }>
}

// Module-scoped — even if multiple plugin instances get created (e.g. a
// renderDiagrams() race before its langCache is populated), every render
// across the whole app gets a unique id, and only one mermaid.render runs
// at a time. Mermaid uses a shared scratch element in document.body for
// layout, so concurrent calls absolutely clobber each other.
let mermaidCounter = 0
let mermaidQueue: Promise<unknown> = Promise.resolve()

const mermaidPlugin: PluginEntry = {
  version: '1',
  load: async () => {
    ensureMermaidCssStyleSheetCompatibility()
    const mod = (await import('mermaid-mini')) as unknown as { default?: MermaidLike }
    const mermaid: MermaidLike =
      (mod.default as MermaidLike | undefined) ?? (mod as unknown as MermaidLike)
    let initialized = false
    const ensureInit = () => {
      if (initialized) return
      // `securityLevel: 'loose'` lets click/href in diagrams pass through.
      // For PDF export this doesn't matter (no interactivity); for in-app
      // RichEditor it does.
      mermaid.initialize({ startOnLoad: false, securityLevel: 'loose' })
      initialized = true
    }

    return {
      async render(source, container) {
        container.innerHTML = ''
        if (!source.trim()) {
          container.innerHTML = '<div class="renderer-empty">Empty diagram</div>'
          return
        }
        ensureInit()
        const id = `mermaid-${++mermaidCounter}-${Date.now()}`
        const job = mermaidQueue.then(async () => {
          try {
            const result = await mermaid.render(id, source)
            container.innerHTML = result.svg
          } catch (e) {
            container.innerHTML = `<div class="renderer-error">${escapeRenderError(e)}</div>`
          }
        })
        // Swallow rejections in the queue so one bad diagram doesn't poison
        // every subsequent render. The job above already converts errors to
        // a renderer-error div, so this catch is just chain-hygiene.
        mermaidQueue = job.catch(() => {})
        return job
      },
      destroy(container) {
        container.innerHTML = ''
      },
    }
  },
}

/** Top-level loader for the dot/graphviz renderer. Used by both the
 *  RichEditor's `rendererRegistry` and the PDF export pipeline. */
export const loadDotRenderer = dotPlugin.load

/** Top-level loader for the mermaid renderer. Used directly by the
 *  host-render-html / PDF / share pipelines via `diagram-render.ts`.
 *  NOT registered in the registry below: @moraya/core has its own built-in
 *  mermaid path (enableMermaid: true → plugins/mermaid-renderer.ts) and
 *  registering 'mermaid' here would make its CodeBlock NodeView treat
 *  mermaid blocks as both isMermaid AND isRenderer, which mis-fires the
 *  `pre.style.display` formula in syncMermaidMode and hides the source
 *  pre when the user clicks Edit — leaving a blank area. */
export const loadMermaidRenderer = mermaidPlugin.load

const PLUGINS: Record<string, PluginEntry> = {
  dot: dotPlugin,
  graphviz: dotPlugin,
}

export class DefaultRendererRegistry implements RendererRegistry {
  readonly versions: Readonly<Record<string, string>> = Object.freeze(
    Object.fromEntries(Object.entries(PLUGINS).map(([id, p]) => [id, p.version])),
  )

  has(language: string): boolean {
    return language in PLUGINS
  }

  async load(language: string): Promise<RendererPluginModule> {
    const entry = PLUGINS[language]
    if (!entry) throw new Error(`[RendererRegistry] no renderer for "${language}"`)
    return entry.load()
  }
}

export const rendererRegistry = new DefaultRendererRegistry()
