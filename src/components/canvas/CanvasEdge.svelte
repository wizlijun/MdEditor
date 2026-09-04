<script lang="ts">
  import { tick } from 'svelte'
  import {
    BaseEdge,
    EdgeLabel,
    EdgeReconnectAnchor,
    getBezierPath,
    type Edge,
    type EdgeProps,
  } from '@xyflow/svelte'

  type CanvasEdgeData = Record<string, unknown> & {
    canonicalId?: string
    interactionLocked?: boolean
    onLabelCommit?: (id: string, value: string) => void
  }

  let {
    id,
    data,
    interactionWidth,
    label,
    markerEnd,
    markerStart,
    selected = false,
    sourcePosition,
    sourceX,
    sourceY,
    style,
    targetPosition,
    targetX,
    targetY,
  }: EdgeProps<Edge<CanvasEdgeData>> = $props()

  let editing = $state(false)
  let draft = $state('')
  let input: HTMLInputElement | undefined = $state()

  let [path, labelX, labelY] = $derived(getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  }))

  async function beginEdit(event?: MouseEvent): Promise<void> {
    event?.preventDefault()
    event?.stopPropagation()
    if (data?.interactionLocked) return
    draft = typeof label === 'string' ? label : ''
    editing = true
    await tick()
    input?.focus()
    input?.select()
  }

  function cancelEdit(): void {
    editing = false
    draft = typeof label === 'string' ? label : ''
  }

  function commitEdit(): void {
    if (!editing) return
    editing = false
    const next = draft.trim()
    if (next === (typeof label === 'string' ? label : '')) return
    data?.onLabelCommit?.((data.canonicalId as string | undefined) ?? id, next)
  }

  function handleInputKeydown(event: KeyboardEvent): void {
    event.stopPropagation()
    if (event.key === 'Enter') {
      event.preventDefault()
      commitEdit()
    } else if (event.key === 'Escape') {
      event.preventDefault()
      cancelEdit()
    }
  }

</script>

<BaseEdge
  {id}
  {path}
  {labelX}
  {labelY}
  {markerStart}
  {markerEnd}
  {interactionWidth}
  {style}
/>

{#if selected || label || editing}
  <EdgeLabel x={labelX} y={labelY} selectEdgeOnClick transparent>
    <div class="canvas-edge-label-content nodrag nopan">
      {#if editing}
        <input
          bind:this={input}
          bind:value={draft}
          data-canvas-edge-label={id}
          aria-label="连线标签"
          placeholder="连线标签"
          onkeydown={handleInputKeydown}
          onblur={commitEdit}
          onclick={(event) => event.stopPropagation()}
        />
      {:else}
        <button
          data-canvas-edge-label={id}
          class:empty={!label}
          aria-label={label ? `编辑连线标签：${label}` : '添加连线标签'}
          title="双击编辑连线标签"
          ondblclick={beginEdit}
        >{label || '＋ 标签'}</button>
      {/if}
    </div>
  </EdgeLabel>
{/if}

{#if selected && !data?.interactionLocked}
  <EdgeReconnectAnchor
    type="source"
    position={{ x: sourceX, y: sourceY }}
    class="canvas-edge-reconnect"
    aria-label="重连起点"
  />
  <EdgeReconnectAnchor
    type="target"
    position={{ x: targetX, y: targetY }}
    class="canvas-edge-reconnect"
    aria-label="重连终点"
  />
{/if}

<style>
  .canvas-edge-label-content {
    transform: scale(var(--canvas-inverse-zoom, 1));
    transform-origin: center;
  }
  button,
  input {
    max-width: 220px;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, Canvas 94%, transparent);
    color: CanvasText;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.12);
    font: inherit;
    font-size: 12px;
    line-height: 1.3;
  }
  button {
    padding: 3px 7px;
    cursor: text;
  }
  button.empty { opacity: 0.7; }
  input {
    width: 180px;
    padding: 5px 7px;
    outline: 2px solid color-mix(in srgb, var(--accent, #4d88ff) 45%, transparent);
  }
</style>
