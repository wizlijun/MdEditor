<script lang="ts">
  import {
    Handle,
    NodeResizer,
    Position,
    type NodeProps,
    type ResizeDragEvent,
    type ResizeParams,
    type ResizeParamsWithDirection,
  } from '@xyflow/svelte'
  import type { MediaResolver } from '@moraya/core'
  import {
    CANVAS_GROUP_MIN_HEIGHT,
    CANVAS_GROUP_MIN_WIDTH,
    CANVAS_NODE_MIN_HEIGHT,
    CANVAS_NODE_MIN_WIDTH,
  } from '../../lib/canvas'
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
    multipleSelected?: boolean
    interactionLocked?: boolean
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
    onResizeStart?: (id: string, rectangle: ResizeParams) => void
    onResize?: (id: string, event: ResizeDragEvent, rectangle: ResizeParamsWithDirection) => void
    onResizeEnd?: (id: string, event: ResizeDragEvent, rectangle: ResizeParams) => void
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
  function mediaResolverRoot(resolver?: MediaResolver): string {
    const root = (resolver as MediaResolver & { root?: unknown } | undefined)?.root
    return typeof root === 'string' ? root : ''
  }
  let editorMountKey = $derived(`${data.canvasPath ?? ''}\u0000${mediaResolverRoot(data.mediaResolver)}`)

  function textSummary(markdown: string): string {
    const firstLine = markdown.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? '文本'
    const plain = firstLine
      .replace(/^#{1,6}\s+/, '')
      .replace(/^[-*+>]\s+/, '')
      .replace(/[`*_~\[\]]/g, '')
      .trim()
    return (plain || '文本').slice(0, 72)
  }
  let title = $derived(
    data.kind === 'text' ? textSummary(data.text ?? '')
      : data.kind === 'file' ? basename(data.file ?? '')
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
  isVisible={selected && !data.multipleSelected && !data.interactionLocked && !data.active && data.kind !== 'opaque'}
  handleClass="canvas-card-resize-handle"
  minWidth={data.kind === 'group' ? CANVAS_GROUP_MIN_WIDTH : CANVAS_NODE_MIN_WIDTH}
  minHeight={data.kind === 'group' ? CANVAS_GROUP_MIN_HEIGHT : CANVAS_NODE_MIN_HEIGHT}
  color={accent ?? 'var(--accent, #4d88ff)'}
  onResizeStart={(_event, rectangle) => data.onResizeStart?.(id, rectangle)}
  onResize={(event, rectangle) => data.onResize?.(id, event, rectangle)}
  onResizeEnd={(event, rectangle) => data.onResizeEnd?.(id, event, rectangle)}
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
  <div class="compact-label" title={title}>{title}</div>
  <div class="node-detail">
    {#if data.kind === 'text'}
      {#if data.active && data.mediaResolver}
        {#key editorMountKey}
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
        {/key}
      {:else}
        <CanvasMarkdownPreview
          markdown={data.text ?? ''}
          resolveLocalResource={data.resolveLocalResource}
        />
      {/if}
    {:else if data.kind === 'file'}
      <div class="card-heading" title={data.file}>{title}</div>
      {#if data.imageUrl}
        <img class="file-image" src={data.imageUrl} alt={title} draggable="false" />
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
    border: 1px solid color-mix(in srgb, CanvasText 14%, transparent);
    border-radius: 12px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--card-accent) 8%, transparent), transparent 54px),
      color-mix(in srgb, Canvas 97%, CanvasText 3%);
    color: CanvasText;
    box-shadow: 0 8px 26px rgba(0, 0, 0, 0.1), 0 1px 3px rgba(0, 0, 0, 0.06);
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .canvas-card::before {
    content: '';
    position: absolute;
    z-index: 2;
    top: 0;
    right: 12px;
    left: 12px;
    height: 2px;
    border-radius: 0 0 999px 999px;
    background: var(--card-accent);
    pointer-events: none;
  }
  .canvas-card:hover {
    border-color: color-mix(in srgb, CanvasText 23%, transparent);
    box-shadow: 0 12px 34px rgba(0, 0, 0, 0.14), 0 2px 5px rgba(0, 0, 0, 0.07);
  }
  .canvas-card.active-editor {
    border-color: var(--accent, #4d88ff);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent, #4d88ff) 24%, transparent),
      0 8px 24px rgba(0, 0, 0, 0.13);
  }
  .node-detail { width: 100%; height: 100%; }
  .compact-label {
    display: none;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    padding: 14px;
    font-size: 18px;
    font-weight: 650;
    line-height: 1.25;
    text-align: center;
    text-overflow: ellipsis;
  }
  .canvas-card.group-node {
    isolation: isolate;
    overflow: visible;
    border: 1.5px dashed color-mix(in srgb, var(--card-accent, CanvasText) 58%, CanvasText 16%);
    background: color-mix(in srgb, var(--card-accent, Canvas) 6%, transparent);
    box-shadow: none;
    pointer-events: none;
  }
  .canvas-card.group-node::before { display: none; }
  .group-label {
    position: absolute;
    z-index: 1;
    top: -31px;
    left: 0;
    max-width: calc(100% - 4px);
    overflow: hidden;
    padding: 5px 9px;
    border: 1px solid color-mix(in srgb, var(--card-accent, CanvasText) 24%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, Canvas 88%, transparent);
    color: color-mix(in srgb, CanvasText 76%, transparent);
    font-size: 12px;
    font-weight: 650;
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.08);
    backdrop-filter: blur(12px);
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
    height: 38px;
    box-sizing: border-box;
    overflow: hidden;
    padding: 11px 13px 8px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 10%, transparent);
    font-size: 12px;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-image {
    width: 100%;
    height: calc(100% - 38px);
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
  .file-placeholder { height: calc(100% - 38px); }
  .file-placeholder > span, .link-icon {
    display: grid;
    width: 42px;
    height: 42px;
    place-items: center;
    border-radius: 12px;
    background: color-mix(in srgb, var(--card-accent, CanvasText) 10%, transparent);
    font-size: 22px;
    opacity: 0.72;
  }
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
    width: 10px;
    height: 10px;
    border: 2.5px solid Canvas;
    background: var(--accent, #4d88ff);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.22);
    opacity: 0;
    transition: opacity 100ms ease, transform 100ms ease;
  }
  :global(.svelte-flow__node:hover .canvas-handle),
  :global(.svelte-flow__node.selected .canvas-handle) { opacity: 1; }
  :global(.canvas-handle:hover) { transform: scale(1.25); }
  :global(.svelte-flow__node-canvas-group .canvas-handle),
  :global(.svelte-flow__node-canvas-group .svelte-flow__resize-control) { pointer-events: auto; }
  @media (pointer: coarse) {
    :global(.canvas-handle) {
      width: 44px;
      height: 44px;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px);
      opacity: 0.72;
    }
    :global(.canvas-card-resize-handle) {
      width: 44px !important;
      height: 44px !important;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px) !important;
    }
  }
</style>
