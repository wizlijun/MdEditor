<script lang="ts">
  import type { CanvasPoint, CanvasRect, ResizeCorner } from '../../lib/canvas'

  interface CanvasViewport {
    x: number
    y: number
    zoom: number
  }

  let {
    bounds,
    viewport,
    onStart,
    onMove,
    onEnd,
    onCancel,
    onKeyboardResize,
  }: {
    bounds: CanvasRect
    viewport: CanvasViewport
    onStart: (corner: ResizeCorner, event: PointerEvent) => void
    onMove: (event: PointerEvent) => void
    onEnd: (event: PointerEvent) => void
    onCancel: (event: PointerEvent) => void
    onKeyboardResize: (corner: ResizeCorner, delta: CanvasPoint) => void
  } = $props()

  const corners: ReadonlyArray<{ corner: ResizeCorner; cursor: string }> = [
    { corner: 'tl', cursor: 'nwse-resize' },
    { corner: 'tr', cursor: 'nesw-resize' },
    { corner: 'bl', cursor: 'nesw-resize' },
    { corner: 'br', cursor: 'nwse-resize' },
  ]

  function start(corner: ResizeCorner, event: PointerEvent): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    try { (event.currentTarget as HTMLElement | null)?.setPointerCapture(event.pointerId) } catch { /* detached handle */ }
    onStart(corner, event)
  }

  function move(event: PointerEvent): void {
    event.preventDefault()
    event.stopPropagation()
    onMove(event)
  }

  function finish(event: PointerEvent, cancelled: boolean): void {
    event.preventDefault()
    event.stopPropagation()
    try { (event.currentTarget as HTMLElement | null)?.releasePointerCapture(event.pointerId) } catch { /* already released */ }
    if (cancelled) onCancel(event)
    else onEnd(event)
  }

  function keydown(corner: ResizeCorner, event: KeyboardEvent): void {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return
    event.preventDefault()
    event.stopPropagation()
    const step = (event.shiftKey ? 10 : 1) / Math.max(viewport.zoom, 0.05)
    onKeyboardResize(corner, {
      x: event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0,
      y: event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0,
    })
  }

  function cornerLabel(corner: ResizeCorner): string {
    return corner === 'tl' ? '左上角' : corner === 'tr' ? '右上角' : corner === 'bl' ? '左下角' : '右下角'
  }

  let screenRect = $derived({
    x: bounds.x * viewport.zoom + viewport.x,
    y: bounds.y * viewport.zoom + viewport.y,
    width: bounds.width * viewport.zoom,
    height: bounds.height * viewport.zoom,
  })
</script>

<div
  class="selection-resizer"
  style:left={`${screenRect.x}px`}
  style:top={`${screenRect.y}px`}
  style:width={`${Math.max(0, screenRect.width)}px`}
  style:height={`${Math.max(0, screenRect.height)}px`}
  role="group"
  aria-label="缩放选区"
>
  {#each corners as { corner, cursor }}
    <button
      type="button"
      class={`resize-handle ${corner}`}
      style:cursor
      aria-label={`缩放选区${cornerLabel(corner)}`}
      onpointerdown={(event) => start(corner, event)}
      onpointermove={move}
      onpointerup={(event) => finish(event, false)}
      onpointercancel={(event) => finish(event, true)}
      onkeydown={(event) => keydown(corner, event)}
    ></button>
  {/each}
</div>

<style>
  .selection-resizer {
    position: absolute;
    z-index: 21;
    box-sizing: border-box;
    border: 1px solid var(--accent, #4d88ff);
    pointer-events: none;
  }
  .resize-handle {
    position: absolute;
    width: 10px;
    height: 10px;
    box-sizing: border-box;
    padding: 0;
    border: 1px solid Canvas;
    border-radius: 2px;
    background: var(--accent, #4d88ff);
    pointer-events: auto;
    touch-action: none;
  }
  .resize-handle:focus-visible {
    outline: 2px solid var(--accent, #4d88ff);
    outline-offset: 3px;
  }
  .resize-handle.tl { top: -5px; left: -5px; }
  .resize-handle.tr { top: -5px; right: -5px; }
  .resize-handle.bl { bottom: -5px; left: -5px; }
  .resize-handle.br { right: -5px; bottom: -5px; }
  @media (pointer: coarse) {
    .resize-handle {
      width: 28px;
      height: 28px;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px);
    }
    .resize-handle.tl { top: -14px; left: -14px; }
    .resize-handle.tr { top: -14px; right: -14px; }
    .resize-handle.bl { bottom: -14px; left: -14px; }
    .resize-handle.br { right: -14px; bottom: -14px; }
  }
</style>
