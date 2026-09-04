<script lang="ts">
  import { renderMarkdownInline } from '../../lib/plugins/host-render-html'

  let {
    markdown,
    resolveLocalResource = async (_src: string) => null,
  }: {
    markdown: string
    resolveLocalResource?: (src: string) => Promise<string | null>
  } = $props()

  const SAFE_TAGS = new Set([
    'A', 'BLOCKQUOTE', 'BR', 'CODE', 'DEL', 'DIV', 'EM', 'H1', 'H2', 'H3',
    'H4', 'H5', 'H6', 'HR', 'IMG', 'LI', 'MARK', 'OL', 'P', 'PRE', 'SPAN',
    'STRONG', 'SUB', 'SUP', 'TABLE', 'TBODY', 'TD', 'TH', 'THEAD', 'TR', 'UL',
  ])
  const SAFE_ATTRS = new Set(['alt', 'class', 'colspan', 'rowspan', 'title'])

  function safeHttpUrl(value: string): string | null {
    try {
      const url = new URL(value)
      return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null
    } catch {
      return null
    }
  }

  async function sanitize(
    html: string,
    resolver: (src: string) => Promise<string | null>,
  ): Promise<string> {
    if (typeof DOMParser === 'undefined') return ''
    const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html')
    const imageLoads: Promise<void>[] = []
    for (const el of Array.from(doc.body.querySelectorAll('*'))) {
      if (!SAFE_TAGS.has(el.tagName)) {
        el.replaceWith(...Array.from(el.childNodes))
        continue
      }
      for (const attr of Array.from(el.attributes)) {
        const name = attr.name.toLowerCase()
        if (!SAFE_ATTRS.has(name) && name !== 'href' && name !== 'src') {
          el.removeAttribute(attr.name)
        }
      }
      if (el.tagName === 'A') {
        const href = safeHttpUrl(el.getAttribute('href') ?? '')
        if (href) {
          el.setAttribute('href', href)
          el.setAttribute('rel', 'noopener noreferrer')
        } else {
          el.removeAttribute('href')
        }
      }
      if (el.tagName === 'IMG') {
        const src = el.getAttribute('src') ?? ''
        el.removeAttribute('src')
        imageLoads.push(resolver(src).then((resolved) => {
          if (resolved) el.setAttribute('src', resolved)
        }).catch(() => {}))
      }
    }
    await Promise.all(imageLoads)
    return doc.body.innerHTML
  }

  let html = $state('')
  $effect(() => {
    const rendered = renderMarkdownInline(markdown)
    const resolver = resolveLocalResource
    let cancelled = false
    void sanitize(rendered, resolver).then((value) => {
      if (!cancelled) html = value
    })
    return () => { cancelled = true }
  })

  function handleClick(event: MouseEvent): void {
    const anchor = (event.target as HTMLElement | null)?.closest('a[href]') as HTMLAnchorElement | null
    if (!anchor) return
    const href = safeHttpUrl(anchor.href)
    event.preventDefault()
    event.stopPropagation()
    if (!href) return
    void import('@tauri-apps/plugin-opener')
      .then(({ openUrl }) => openUrl(href))
      .catch((error) => console.warn('[CanvasMarkdownPreview] open link failed:', error))
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<article
  class="canvas-markdown-preview moraya-editor nodrag nopan nowheel"
  onclick={handleClick}
>
  {@html html}
</article>

<style>
  .canvas-markdown-preview {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    overflow: auto;
    padding: 12px 14px;
    color: inherit;
    background: transparent;
    font-size: 14px;
    line-height: 1.5;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .canvas-markdown-preview :global(> :first-child) { margin-top: 0; }
  .canvas-markdown-preview :global(> :last-child) { margin-bottom: 0; }
  .canvas-markdown-preview :global(img) {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  .canvas-markdown-preview :global(pre) {
    overflow: auto;
    white-space: pre-wrap;
  }
  .canvas-markdown-preview :global(table) {
    display: block;
    max-width: 100%;
    overflow: auto;
  }
</style>
