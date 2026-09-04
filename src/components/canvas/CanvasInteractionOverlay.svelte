<script lang="ts">
  import type { CanvasPoint, SnapGuide } from '../../lib/canvas'

  interface CanvasViewport {
    x: number
    y: number
    zoom: number
  }

  interface ScreenRect {
    x: number
    y: number
    width: number
    height: number
  }

  let {
    guides,
    lassoPoints,
    viewport,
  }: {
    guides: SnapGuide[]
    lassoPoints: CanvasPoint[]
    viewport: CanvasViewport
  } = $props()

  function screenX(x: number): number {
    return x * viewport.zoom + viewport.x
  }

  function screenY(y: number): number {
    return y * viewport.zoom + viewport.y
  }

  function projectRect(rect: { x: number; y: number; width: number; height: number }): ScreenRect {
    return {
      x: screenX(rect.x),
      y: screenY(rect.y),
      width: rect.width * viewport.zoom,
      height: rect.height * viewport.zoom,
    }
  }

  function tickFor(
    guide: Extract<SnapGuide, { kind: 'equal-spacing' }>,
    index: number,
  ): { x1: number; y1: number; x2: number; y2: number } {
    const leading = projectRect(guide.rects[index])
    const trailing = projectRect(guide.rects[index + 1])
    const tickHalfLength = 6

    if (guide.axis === 'x') {
      const x = (leading.x + leading.width + trailing.x) / 2
      const y = screenY(guide.value)
      return { x1: x, y1: y - tickHalfLength, x2: x, y2: y + tickHalfLength }
    }

    const x = screenX(guide.value)
    const y = (leading.y + leading.height + trailing.y) / 2
    return { x1: x - tickHalfLength, y1: y, x2: x + tickHalfLength, y2: y }
  }

  let lassoPolygon = $derived(lassoPoints.map((point) => `${point.x},${point.y}`).join(' '))
</script>

{#if guides.length > 0 || lassoPoints.length > 1}
  <svg class="canvas-interaction-overlay" aria-hidden="true">
    {#each guides as guide, index (`${guide.kind}-${guide.axis}-${guide.value}-${index}`)}
      {@const vertical = guide.kind === 'alignment' ? guide.axis === 'x' : guide.axis === 'y'}
      {@const x1 = vertical ? screenX(guide.value) : screenX(guide.from)}
      {@const y1 = vertical ? screenY(guide.from) : screenY(guide.value)}
      {@const x2 = vertical ? screenX(guide.value) : screenX(guide.to)}
      {@const y2 = vertical ? screenY(guide.to) : screenY(guide.value)}
      <g>
        <line
          {x1}
          {y1}
          {x2}
          {y2}
          class:equal-spacing={guide.kind === 'equal-spacing'}
          class="snap-guide"
        />
        {#if guide.kind === 'equal-spacing'}
          {#each guide.rects.slice(0, -1) as _, rectIndex}
            {@const tick = tickFor(guide, rectIndex)}
            <line
              x1={tick.x1}
              y1={tick.y1}
              x2={tick.x2}
              y2={tick.y2}
              class="spacing-tick"
            />
          {/each}
        {/if}
      </g>
    {/each}

    {#if lassoPoints.length > 1}
      <polygon class="lasso-polygon" points={lassoPolygon} />
    {/if}
  </svg>
{/if}

<style>
  .canvas-interaction-overlay {
    position: absolute;
    z-index: 19;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }

  .snap-guide,
  .spacing-tick {
    fill: none;
    stroke: var(--accent, #4d88ff);
    vector-effect: non-scaling-stroke;
  }

  .snap-guide {
    stroke-width: 1;
  }

  .snap-guide.equal-spacing {
    stroke-dasharray: 4 3;
  }

  .spacing-tick {
    stroke-width: 1.5;
  }

  .lasso-polygon {
    fill: color-mix(in srgb, var(--accent, #4d88ff) 14%, transparent);
    stroke: var(--accent, #4d88ff);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
</style>
