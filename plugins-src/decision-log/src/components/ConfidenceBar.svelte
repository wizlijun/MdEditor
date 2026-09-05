<!-- ConfidenceBar.svelte — confidence picker as a 10-cell probability meter
     (v1.1, supersedes the star rating). Cells sit edge-to-edge as one
     continuous bar spanning the full width. The first 5 cells are always
     filled: they are the 0–50% baseline you get "for free" — a decision you
     sign is, by definition, one you believe more likely than not. The last 5
     cells are selectable like a progress bar; each is one confidence level
     anchored to a probability (55/65/75/85/95%). The header row carries the
     field label on the left and the hovered/selected cell's caption on the
     RIGHT half — it starts at the 50% mark, i.e. above the selectable cells.
     Internally the 5 levels reuse the model's star-level math (starOf/anchorOf);
     only the rendering changed from glyphs to cells. Readonly mode (cards,
     locked verdict view) renders the same meter without buttons. -->
<script lang="ts">
  import { anchorOf, starOf, type Confidence } from '../lib/model'
  import { starLabel } from '../lib/strings'

  let {
    value = null,
    readonly = false,
    compact = false,
    label = '',
    required = false,
    onChange,
  }: {
    /** Numeric confidence (0–1) or null (unset). */
    value?: Confidence | null
    readonly?: boolean
    compact?: boolean
    /** Field label shown on the left of the header row (left 50%). */
    label?: string
    required?: boolean
    onChange?: (confidence: Confidence) => void
  } = $props()

  // Selected level 0..5 in the upper (selectable) half.
  const level = $derived(value == null ? 0 : starOf(value))
  let hovered = $state(0) // 1..5 selectable cell under the pointer/focus
  const shown = $derived(hovered || level)

  // Caption follows the hovered cell live, else the current selection.
  const active = $derived(hovered || level)
  const caption = $derived(
    active === 0 ? '' : `${starLabel(active)} · ≈${Math.round(anchorOf(active) * 100)}%`
  )
</script>

<div class="cbar" class:compact class:ro={readonly} title={readonly ? caption : undefined}>
  {#if !compact}
    <div class="head">
      <span class="lbl">{label}{#if required} <span class="req">*</span>{/if}</span>
      <span class="caption" aria-live="polite">{caption}</span>
    </div>
  {/if}
  <div class="cells" role="group" aria-label={label || caption || undefined} onpointerleave={() => (hovered = 0)}>
    {#each [1, 2, 3, 4, 5] as _b (`b${_b}`)}
      <span class="cell base"></span>
    {/each}
    {#each [1, 2, 3, 4, 5] as sel (`u${sel}`)}
      {#if readonly}
        <span class="cell up" class:on={sel <= level} class:mid={sel === 1}></span>
      {:else}
        <button
          type="button"
          class="cell up"
          class:on={sel <= shown}
          class:mid={sel === 1}
          aria-label={`${starLabel(sel)} ≈${Math.round(anchorOf(sel) * 100)}%`}
          aria-pressed={sel === level}
          onpointerenter={() => (hovered = sel)}
          onfocus={() => (hovered = sel)}
          onblur={() => (hovered = 0)}
          onclick={() => onChange?.(anchorOf(sel))}
        ></button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .cbar { display: flex; flex-direction: column; gap: 0.3rem; }
  /* full-width continuous bar; cells touch edge-to-edge (no gaps) */
  .cells { display: flex; width: 100%; align-items: stretch; overflow: hidden; border-radius: 4px; }
  .cell {
    flex: 1 1 0; min-width: 0; height: 20px; border: 0; padding: 0;
  }
  /* hairline separator between touching cells (keeps 10 segments countable) */
  .cell + .cell { box-shadow: inset 1px 0 0 0 color-mix(in srgb, Canvas 55%, transparent); }
  /* baseline (0–50%): muted, non-interactive */
  .cell.base { background: color-mix(in srgb, currentColor 22%, transparent); }
  /* selectable upper half */
  .cell.up {
    background: color-mix(in srgb, currentColor 9%, transparent);
    cursor: pointer; transition: background 0.1s;
  }
  .cell.up.on { background: var(--accent, #2563eb); }
  /* slightly stronger divider at the 50% mark (first selectable cell) */
  .cell.up.mid { box-shadow: inset 2px 0 0 0 color-mix(in srgb, Canvas 80%, transparent); }
  button.cell.up:hover { filter: brightness(1.08); }
  button.cell.up:focus-visible { outline: 2px solid var(--accent, #2563eb); outline-offset: 1px; }
  .cbar.ro .cell.up { cursor: default; }
  /* header row: label on the left half, caption on the right half (starts at
     the 50% mark → sits above the selectable cells). */
  .head { display: flex; align-items: baseline; min-height: 1.2em; }
  .lbl { flex: 0 0 50%; font-size: 0.8rem; opacity: 0.7; }
  .req { color: var(--ui-danger); }
  .caption {
    flex: 1 1 50%; font-size: 0.82rem; opacity: 0.78; font-variant-numeric: tabular-nums;
  }

  /* compact (cards): shorter cells, no caption, natural (not full) width */
  .compact { display: inline-flex; }
  .compact .cells { width: auto; border-radius: 3px; }
  .compact .cell { flex: 0 0 auto; width: 9px; height: 11px; }
</style>
