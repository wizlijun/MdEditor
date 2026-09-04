<script lang="ts">
  import { onMount } from 'svelte'
  import type { MediaResolver } from '@moraya/core'
  import { CanvasMarkdownResourceGuard, containsRemoteMediaHtml } from '../../lib/canvas/markdown-security'

  let {
    markdown,
    tabId = '',
    filePath,
    mediaResolver,
    onChange,
    onFlush,
    onBlur = (_markdown: string) => {},
    onCompositionChange = (_composing: boolean) => {},
  }: {
    markdown: string
    tabId?: string
    filePath: string
    mediaResolver: MediaResolver
    onChange: (markdown: string) => void
    onFlush: (markdown: string) => void
    onBlur?: (markdown: string) => void
    onCompositionChange?: (composing: boolean) => void
  } = $props()

  let host: HTMLDivElement | undefined = $state()
  let status: 'loading' | 'ready' | 'error' = $state('loading')
  let errorMessage = $state('')
  let editor: Awaited<ReturnType<typeof import('../../lib/editor-bridge')['mountRichEditor']>> | null = null
  const resourceGuard = new CanvasMarkdownResourceGuard()
  function initialMarkdown(): string { return markdown }
  let lastMarkdown = initialMarkdown()
  let destroyed = false
  let isComposing = false

  function flush(): void {
    if (!editor) return
    const value = resourceGuard.restore(editor.getMarkdown())
    lastMarkdown = value
    onFlush(value)
  }

  function handleCompositionStart(): void {
    isComposing = true
    onCompositionChange(true)
  }

  function handleCompositionEnd(): void {
    isComposing = false
    onCompositionChange(false)
    queueMicrotask(flush)
  }

  function handleFlushRequest(event: Event): void {
    const requestedTabId = (event as CustomEvent<{ tabId?: string }>).detail?.tabId
    if (requestedTabId && requestedTabId !== tabId) return
    flush()
  }

  function handleFocusOut(event: FocusEvent): void {
    const next = event.relatedTarget as Node | null
    if (next && event.currentTarget instanceof Node && event.currentTarget.contains(next)) return
    queueMicrotask(() => {
      if (!editor) return
      const value = resourceGuard.restore(editor.getMarkdown())
      lastMarkdown = value
      onFlush(value)
      onBlur(value)
    })
  }

  function handlePasteCapture(event: ClipboardEvent): void {
    const html = event.clipboardData?.getData('text/html') ?? ''
    if (!containsRemoteMediaHtml(html)) return
    event.preventDefault()
    event.stopImmediatePropagation()
    const text = event.clipboardData?.getData('text/plain') ?? ''
    if (text) document.execCommand?.('insertText', false, text)
  }

  onMount(() => {
    if (!host) return
    const root = host
    root.addEventListener('paste', handlePasteCapture, true)
    window.addEventListener('notemd:flush-doc', handleFlushRequest)
    void (async () => {
      try {
        // The main EditorPane renders exactly one surface. CanvasView also keeps
        // exactly one embedded editor alive and destroys it before switching
        // cards, so the legacy module-level base directory never has competing
        // owners in this webview.
        const { mountRichEditor, updateDocumentBaseDir } = await import('../../lib/editor-bridge')
        updateDocumentBaseDir(filePath)
        const mounted = await mountRichEditor(root, resourceGuard.shield(markdown), (value) => {
          const restored = resourceGuard.restore(value)
          lastMarkdown = restored
          onChange(restored)
        }, undefined, mediaResolver)
        if (destroyed) {
          mounted.destroy()
          return
        }
        editor = mounted
        status = 'ready'
        queueMicrotask(() => mounted.view.focus())
      } catch (error) {
        status = 'error'
        errorMessage = error instanceof Error ? error.message : String(error)
      }
    })()

    return () => {
      destroyed = true
      root.removeEventListener('paste', handlePasteCapture, true)
      window.removeEventListener('notemd:flush-doc', handleFlushRequest)
      if (!editor) return
      const value = resourceGuard.restore(editor.getMarkdown())
      if (value !== lastMarkdown) onChange(value)
      if (isComposing) {
        isComposing = false
        onCompositionChange(false)
      }
      onFlush(value)
      editor.destroy()
      editor = null
    }
  })

  $effect(() => {
    const incoming = markdown
    if (!editor || incoming === lastMarkdown) return
    lastMarkdown = incoming
    editor.setContent(resourceGuard.shield(incoming))
  })
</script>

<div
  class="embedded-markdown nodrag nopan nowheel"
  bind:this={host}
  oncompositionstart={handleCompositionStart}
  oncompositionend={handleCompositionEnd}
  onfocusout={handleFocusOut}
  aria-busy={status === 'loading'}
>
  {#if status === 'loading'}
    <div class="editor-status">正在载入编辑器…</div>
  {:else if status === 'error'}
    <div class="editor-status error">编辑器载入失败：{errorMessage}</div>
  {/if}
</div>

<style>
  .embedded-markdown {
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: auto;
    background: var(--canvas-node-bg, Canvas);
  }
  .embedded-markdown :global(.ProseMirror) {
    box-sizing: border-box;
    min-height: 100%;
    padding: 12px 14px;
    outline: none;
  }
  .editor-status {
    padding: 12px;
    color: color-mix(in srgb, CanvasText 55%, transparent);
    font-size: 12px;
  }
  .editor-status.error { color: #c13f3f; }
</style>
