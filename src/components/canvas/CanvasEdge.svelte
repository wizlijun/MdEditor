<script lang="ts">
  import {
    BaseEdge,
    EdgeReconnectAnchor,
    getBezierPath,
    type EdgeProps,
  } from '@xyflow/svelte'

  let {
    id,
    interactionWidth,
    label,
    labelStyle,
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
  }: EdgeProps = $props()

  let [path, labelX, labelY] = $derived(getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  }))
</script>

<BaseEdge
  {id}
  {path}
  {labelX}
  {labelY}
  {label}
  {labelStyle}
  {markerStart}
  {markerEnd}
  {interactionWidth}
  {style}
/>

{#if selected}
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
