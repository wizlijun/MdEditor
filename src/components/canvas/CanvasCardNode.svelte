<script lang="ts">
  import { Handle, NodeResizer, Position, type NodeProps, type ResizeParams } from '@xyflow/svelte'
  import type { MediaResolver } from '@moraya/core'
  import { basename } from '../../lib/paths'
  import CanvasMarkdownPreview from './CanvasMarkdownPreview.svelte'
  import EmbeddedMarkdownEditor from './EmbeddedMarkdownEditor.svelte'

  interface CanvasCardData {
    kind: 'text' | 'file' | 'link' | 'group' | 'opaque'
    text?: string
    file?: string
    url?: string
    label?: string
    color?: string
    diagnostic?: string
    imageUrl?: string | null
    backgroundUrl?: string | null
    backgroundStyle?: string
    active?: boolean
    tabId?: string
    canvasPath?: string
    mediaResolver?: MediaResolver
    resolveLocalResource?: (src: string) => Promise<string | null>
    onActivate?: (id: string) => void
    onOpen?: (id: string) => void
    onTextChange?: (id: string, markdown: string) => void
    onTextFlush?: (id: string, markdown: string) => void
    onTextBlur?: (id: string, markdown: string) => void
    onCompositionChange?: (composing: boolean) => void
    onResizeEnd?: (id: string, rectangle: ResizeParams) => void
  }

  let { id, data, selected = false }: NodeProps & { data: CanvasCardData } = $props()

  const HANDLE_SIDES = [
    ['side:top', Position.Top],
    ['side:right', Position.Right],
    ['side:bottom', Position.Bottom],
    ['side:left', Position.Left],
  ] as const

  function colorValue(token?: string): string | null {
    const palette: Record<string, string> = {
      '1': '#e05252', '2': '#e08a32', '3': '#d4ae35',
      '4': '#4b9d62', '5': '#3d91a6', '6': '#8066bd',
    }
    if (!token) return null
    if (palette[token]) return palette[token]
    return /^#[0-9a-f]{3,8}$/i.test(token) && [4, 5, 7, 9].includes(token.length)
      ? token
      : null
  }

  let accent = $derived(colorValue(data.color))
  let title = $derived(
    data.kind === 'file' ? basename(data.file ?? '')
      : data.kind === 'link' ? (data.url ?? '链接')
        : data.kind === 'group' ? (data.label ?? '分组')
          : '未知节点',
  )

  function activate(event: MouseEvent): void {
    event.stopPropagation()
    if (data.kind === 'text') data.onActivate?.(id)
    else if (data.kind === 'file' || data.kind === 'link') data.onOpen?.(id)
  }
</script>

<NodeResizer
  isVisible={selected && !data.active && data.kind !== 'opaque'}
  minWidth={data.kind === 'group' ? 180 : 160}
  minHeight={data.kind === 'group' ? 120 : 100}
  color={accent ?? 'var(--accent, #4d88ff)'}
  onResizeEnd={(_event, rectangle) => data.onResizeEnd?.(id, rectangle)}
/>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class:group-node={data.kind === 'group'}
  class:active-editor={data.active}
  class:opaque-node={data.kind === 'opaque'}
  class="canvas-card"
  style:--card-accent={accent ?? 'transparent'}
  ondblclick={activate}
>
  {#if data.kind === 'text'}
    {#if data.active && data.mediaResolver}
      <EmbeddedMarkdownEditor
        markdown={data.text ?? ''}
        tabId={data.tabId ?? ''}
        filePath={data.canvasPath ?? ''}
        mediaResolver={data.mediaResolver}
        onChange={(markdown) => data.onTextChange?.(id, markdown)}
        onFlush={(markdown) => data.onTextFlush?.(id, markdown)}
        onBlur={(markdown) => data.onTextBlur?.(id, markdown)}
        onCompositionChange={(composing) => data.onCompositionChange?.(composing)}
      />
    {:else}
      <CanvasMarkdownPreview
        markdown={data.text ?? ''}
        resolveLocalResource={data.resolveLocalResource}
      />
    {/if}
  {:else if data.kind === 'file'}
    <div class="card-heading" title={data.file}>{title}</div>
    {#if data.imageUrl}
      <img class="file-image nodrag nopan" src={data.imageUrl} alt={title} draggable="false" />
    {:else}
      <div class="file-placeholder">
        <span aria-hidden="true">▤</span>
        <small>{data.file}</small>
      </div>
    {/if}
  {:else if data.kind === 'link'}
    <div class="link-card">
      <span class="link-icon" aria-hidden="true">↗</span>
      <strong title={data.url}>{title}</strong>
      <small>{data.url}</small>
    </div>
  {:else if data.kind === 'group'}
    {#if data.backgroundUrl}
      <div
        class="group-background"
        class:cover={data.backgroundStyle === 'cover'}
        class:ratio={data.backgroundStyle === 'ratio'}
        class:repeat={data.backgroundStyle === 'repeat'}
        style:background-image={`url("${data.backgroundUrl}")`}
      ></div>
    {/if}
    <div class="group-label">{title}</div>
  {:else}
    <div class="opaque-placeholder">
      <strong>无法编辑的节点</strong>
      <small>{data.diagnostic ?? '包含不兼容的 JSON Canvas 字段'}</small>
    </div>
  {/if}
</div>

{#if data.kind !== 'opaque'}
  {#each HANDLE_SIDES as [handleId, position]}
    <Handle id={handleId} type="source" {position} class="canvas-handle" />
  {/each}
{/if}

<style>
  .canvas-card {
    position: relative;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, CanvasText 17%, transparent);
    border-left: 4px solid var(--card-accent);
    border-radius: 10px;
    background: color-mix(in srgb, Canvas 96%, CanvasText 4%);
    color: CanvasText;
    box-shadow: 0 5px 18px rgba(0, 0, 0, 0.11);
  }
  .canvas-card.active-editor {
    border-color: var(--accent, #4d88ff);
    border-left-color: var(--accent, #4d88ff);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent, #4d88ff) 24%, transparent),
      0 8px 24px rgba(0, 0, 0, 0.13);
  }
  .canvas-card.group-node {
    isolation: isolate;
    overflow: visible;
    border: 2px solid color-mix(in srgb, var(--card-accent, CanvasText) 58%, CanvasText 16%);
    border-left-width: 2px;
    background: color-mix(in srgb, var(--card-accent, Canvas) 7%, transparent);
    box-shadow: none;
    pointer-events: none;
  }
  .group-label {
    position: absolute;
    z-index: 1;
    top: -27px;
    left: 2px;
    max-width: calc(100% - 4px);
    overflow: hidden;
    color: color-mix(in srgb, CanvasText 72%, transparent);
    font-size: 13px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: grab;
    pointer-events: auto;
    user-select: none;
  }
  .group-label:active { cursor: grabbing; }
  .group-background {
    position: absolute;
    z-index: 0;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    background-position: center;
    background-repeat: no-repeat;
    background-size: contain;
    opacity: 0.72;
  }
  .group-background.cover { background-size: cover; }
  .group-background.ratio { background-size: contain; }
  .group-background.repeat {
    background-position: left top;
    background-repeat: repeat;
    background-size: auto;
  }
  .card-heading {
    height: 34px;
    box-sizing: border-box;
    overflow: hidden;
    padding: 9px 12px 7px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 10%, transparent);
    font-size: 12px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-image {
    width: 100%;
    height: calc(100% - 34px);
    object-fit: contain;
    background: color-mix(in srgb, Canvas 88%, CanvasText 12%);
  }
  .file-placeholder, .opaque-placeholder, .link-card {
    display: flex;
    height: 100%;
    box-sizing: border-box;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 16px;
    text-align: center;
  }
  .file-placeholder { height: calc(100% - 34px); }
  .file-placeholder > span, .link-icon { font-size: 28px; opacity: 0.6; }
  .file-placeholder small, .opaque-placeholder small, .link-card small {
    width: 100%;
    overflow: hidden;
    color: color-mix(in srgb, CanvasText 55%, transparent);
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  .link-card strong {
    width: 100%;
    overflow: hidden;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opaque-node {
    border-style: dashed;
    background: color-mix(in srgb, #c88a36 9%, Canvas);
  }
  :global(.canvas-handle) {
    width: 9px;
    height: 9px;
    border: 2px solid Canvas;
    background: var(--accent, #4d88ff);
    opacity: 0;
    transition: opacity 100ms ease;
  }
  :global(.svelte-flow__node:hover .canvas-handle),
  :global(.svelte-flow__node.selected .canvas-handle) { opacity: 1; }
  :global(.svelte-flow__node-canvas-group .canvas-handle),
  :global(.svelte-flow__node-canvas-group .svelte-flow__resize-control) { pointer-events: auto; }
  @media (pointer: coarse) {
    :global(.canvas-handle) {
      width: 28px;
      height: 28px;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px);
      opacity: 0.72;
    }
  }
</style>
