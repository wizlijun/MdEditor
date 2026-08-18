<script lang="ts">
  // THE agent picker. One control, one behaviour, wherever an "run this with an
  // agent" button appears: the sidecar note's Answer button, Idea Spark's
  // delegate button, the ebook queue's AI-read button.
  //
  // Shape: `[ Answer ] by Claude ▾`. The button does the work; the picker only
  // says who will do it. They are deliberately separate controls — merging them
  // into a split-button makes "change the agent" and "start a run" one pixel
  // apart, and starting a run by accident costs real tokens.
  //
  // The menu carries the details that decide the choice — version, model, and
  // whether the harness can run at all — because "Claude or DeepSeek" is not
  // enough to choose between them when one of the two is broken.
  //
  // CANONICAL COPY: src/lib/agent-picker/AgentPicker.svelte. The plugin copies
  // are kept byte-identical by a test (see agent-picker.copies.test.ts); edit
  // this file and rerun `node scripts/sync-agent-picker.mjs`.
  import type { AgentOption } from './types'

  let { options, selected, disabled = false, onselect, label }: {
    options: AgentOption[]
    /** Currently chosen provider id. */
    selected: string | null
    disabled?: boolean
    onselect: (id: string) => void
    /** `(key, vars?) => string`, from whichever i18n the surface uses. */
    label: (k: string, v?: Record<string, string | number>) => string
  } = $props()

  let open = $state(false)
  let root: HTMLElement | undefined = $state()

  const current = $derived(options.find((o) => o.id === selected) ?? options[0])
  /** "by Claude" — the short name, because the row is not the place for detail. */
  const shortName = (o: AgentOption | undefined) =>
    o ? (o.harness?.harness ?? o.name).replace(/\s*(Code|Harness)$/i, '') : ''

  /** "2.1.233 · model claude-opus-5" — the line under each menu entry. */
  function detail(o: AgentOption): string {
    if (!o.harness) return label('agentPicker.unknown')
    if (!o.harness.ok) {
      return o.harness.origin
        ? label('agentPicker.broken')
        : label('agentPicker.notInstalled')
    }
    return [
      o.harness.version,
      o.harness.default_model && label('agentPicker.model', { model: o.harness.default_model }),
    ]
      .filter(Boolean)
      .join(' · ')
  }

  function choose(id: string) {
    onselect(id)
    open = false
  }

  // Click-away and Escape. A menu you cannot dismiss without choosing is a
  // menu that has taken a decision hostage.
  $effect(() => {
    if (!open) return
    const away = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false
    }
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false
    }
    document.addEventListener('mousedown', away, true)
    document.addEventListener('keydown', key)
    return () => {
      document.removeEventListener('mousedown', away, true)
      document.removeEventListener('keydown', key)
    }
  })
</script>

<!-- One installed agent is not a choice; showing a menu that can only confirm
     what is already true is noise. State it and stop. -->
{#if options.length <= 1}
  <span class="by one" title={current ? detail(current) : ''}>
    {label('agentPicker.by', { name: shortName(current) })}
  </span>
{:else}
  <span class="wrap" bind:this={root}>
    <button
      class="by"
      {disabled}
      aria-haspopup="menu"
      aria-expanded={open}
      title={current ? detail(current) : ''}
      onclick={() => (open = !open)}
    >
      {label('agentPicker.by', { name: shortName(current) })}
      <span class="caret" aria-hidden="true">▾</span>
    </button>

    {#if open}
      <div class="menu" role="menu">
        {#each options as o (o.id)}
          <button
            role="menuitemradio"
            aria-checked={o.id === selected}
            class="item"
            class:bad={o.harness ? !o.harness.ok : false}
            onclick={() => choose(o.id)}
          >
            <span class="tick" aria-hidden="true">{o.id === selected ? '✓' : ''}</span>
            <span class="text">
              <span class="nm">{o.harness?.harness ?? o.name}</span>
              <span class="dt">{detail(o)}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </span>
{/if}

<style>
  .wrap { position: relative; display: inline-flex; }
  /* button inherits neither font-size nor family — declare both, or the row
     drifts out of alignment at larger UI font sizes. */
  .by {
    font: inherit;
    font-size: 11px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 5px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
    white-space: nowrap;
  }
  .by:hover:not(:disabled) {
    opacity: 0.9;
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .by:disabled { cursor: default; opacity: 0.35; }
  .one { cursor: default; padding: 2px 0; }
  .caret { font-size: 9px; opacity: 0.8; }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 50;
    min-width: 210px;
    padding: 4px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    background: var(--bg-color, Canvas);
    color: inherit;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.22);
  }
  .item {
    font: inherit;
    display: flex;
    align-items: flex-start;
    gap: 6px;
    width: 100%;
    padding: 5px 7px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover { background: color-mix(in srgb, currentColor 12%, transparent); }
  .tick { flex: none; width: 11px; font-size: 11px; line-height: 1.5; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .nm { font-size: 12px; }
  .dt {
    font-size: 10px;
    opacity: 0.6;
    font-family: ui-monospace, SFMono-Regular, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A harness that cannot run is still selectable — you may be about to fix it
     — but it must not look like a working one. */
  .item.bad .dt { color: #d9534f; opacity: 0.95; }
</style>
