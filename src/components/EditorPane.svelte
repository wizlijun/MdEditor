<script lang="ts">
  import type { Tab } from '../lib/tabs.svelte'
  import { setContent } from '../lib/tabs.svelte'
  import RichEditor from './RichEditor.svelte'
  import CsvEditor from './CsvEditor.svelte'
  import BaseView from './BaseView.svelte'
  import CustomEditorIframe from './CustomEditorIframe.svelte'
  import SourceView from './SourceView.svelte'
  import HtmlPreview from './HtmlPreview.svelte'
  import ExternalChangeBanner from './ExternalChangeBanner.svelte'
  import OutlineEditor from './outline/OutlineEditor.svelte'
  import { isOutlineNoteTab } from '../lib/outline/gate.svelte'
  import SyncOriginBanner from './SyncOriginBanner.svelte'
  import MirrorSiblingsBanner from './MirrorSiblingsBanner.svelte'
  import SyncToVaultBanner from './SyncToVaultBanner.svelte'
  import { offsetToLineCol, lineColToOffset } from '../lib/cursor-preserve'
  import { captureScroll } from '../lib/scroll-keep'
  import { convertFileSrc } from '@tauri-apps/api/core'
  import { migrateTempResources, getTempDir } from '../lib/paste-resources'

  let { tab }: { tab: Tab } = $props()

  /** 编辑器栈根元素:重载时在其内部找滚动容器做保位,避免跨面板误伤 */
  let stackEl: HTMLDivElement | undefined = $state()

  // ── Clipboard resource migration ──
  // When an untitled doc is first saved, move pasted temp resources to
  // {docBasename}_files/ and update markdown refs. Runs in the shared
  // parent so it fires regardless of which editor mode is active.
  const _mountedWithPath = !!tab.filePath
  let _didMigrate = false

  $effect(() => {
    const fp = tab.filePath
    if (_mountedWithPath || _didMigrate || !fp) return
    _didMigrate = true
    void (async () => {
      try {
        const tempDir = await getTempDir()
        const snapshot = tab.currentContent
        const updated = await migrateTempResources(snapshot, tempDir, fp)
        if (updated !== snapshot) setContent(tab.id, updated)
      } catch (e) {
        console.warn('[EditorPane] resource migration failed:', e)
      }
    })()
  })

  function onSourceInput(e: Event) {
    const ta = e.currentTarget as HTMLTextAreaElement
    setContent(tab.id, ta.value)
  }

  function onRichFlush(md: string) {
    setContent(tab.id, md)
  }

  // Best-effort cursor preservation when an external change auto-reloads
  // a clean source-mode tab. Re-find the cursor's (line, col) in the new
  // content and reapply. Rich/HTML modes are not handled — their DOM
  // re-renders fully and accurate cursor mapping is out of scope.
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as
        | { tabId: string; oldContent: string; newContent: string }
        | undefined
      if (!detail || detail.tabId !== tab.id) return
      const ta = document.querySelector<HTMLTextAreaElement>(
        `textarea.src-textarea[data-tab-id="${tab.id}"]`,
      )
      // 滚动保位:重载把内容整体换掉,浏览器会把滚动清零。source 滚的是 textarea 本身,
      // rich 滚的是 .scroll 容器。在 stack 内查询,避免跨面板误伤。
      const scroller = tab.mode === 'source'
        ? ta
        : stackEl?.querySelector<HTMLElement>('.rich-pane .scroll') ?? null
      const restoreScroll = captureScroll(scroller)

      if (tab.mode === 'source' && ta) {
        const lc = offsetToLineCol(detail.oldContent, ta.selectionStart)
        const off = lineColToOffset(detail.newContent, lc.line, lc.col)
        // Wait one tick for the bound textarea value to refresh
        queueMicrotask(() => { ta.selectionStart = ta.selectionEnd = off })
      }
      restoreScroll()
    }
    window.addEventListener('notemd:auto-reloaded', handler)
    return () => window.removeEventListener('notemd:auto-reloaded', handler)
  })
</script>

<div class="editor-stack" bind:this={stackEl}>
  <ExternalChangeBanner {tab} />
  <SyncOriginBanner {tab} />
  <MirrorSiblingsBanner {tab} />
  <SyncToVaultBanner {tab} />
  {#if tab.kind === 'image'}
    {#key tab.id}
      <div class="image-preview-wrap">
        <img
          class="image-preview"
          src={`${convertFileSrc(tab.filePath)}?v=${tab.lastKnownMtime}`}
          alt={tab.title}
        />
      </div>
    {/key}
  {:else if tab.kind === 'spreadsheet' && tab.mode !== 'source'}
    {#key tab.id}
      <CsvEditor {tab} />
    {/key}
  {:else if tab.kind === 'base' && tab.mode !== 'source'}
    {#key tab.id}
      <BaseView {tab} />
    {/key}
  {:else if tab.kind === 'custom'}
    {#key tab.id}
      <CustomEditorIframe {tab} />
    {/key}
  {:else if tab.mode === 'source'}
    {#key tab.id}
      <SourceView value={tab.currentContent} oninput={onSourceInput} tabId={tab.id} filePath={tab.filePath} />
    {/key}
  {:else if isOutlineNoteTab(tab)}
    {#key tab.id}
      <OutlineEditor {tab} />
    {/key}
  {:else if tab.kind === 'html'}
    {#key tab.id}
      <HtmlPreview html={tab.currentContent} />
    {/key}
  {:else if tab.kind === 'mdx'}
    <!--
      mdx rich mode = read-only reading view. MDX is markdown + JSX, needs its
      own build pipeline, and is usually somebody's build source; a ProseMirror
      round-trip would mangle its `import` lines and JSX blocks. Editing goes
      through source mode (Cmd+/), which saves byte-for-byte. No annotation and
      no sidecar note — read-only rendering is the whole support surface.
    -->
    {#key tab.id}
      <RichEditor {tab} readOnly />
    {/key}
  {:else}
    {#key tab.id}
      <RichEditor
        {tab}
        onFlush={onRichFlush}
        wrapAsCodeBlock={tab.kind === 'code' ? (tab.language ?? '') : undefined}
      />
    {/key}
  {/if}
</div>

<style>
  .editor-stack {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .image-preview-wrap {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: auto;
    background: color-mix(in srgb, Canvas 92%, CanvasText 8%);
    padding: 24px;
  }
  .image-preview {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
</style>
