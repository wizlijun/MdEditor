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
  import { tick } from 'svelte'
  import { placeMenu, type AgentOption, type Placement } from './types'

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
  let trigger: HTMLButtonElement | undefined = $state()
  let menuEl: HTMLElement | undefined = $state()
  /** Viewport coordinates; null until the menu has been measured. */
  let at: Placement | null = $state(null)

  /**
   * Measure the button and the menu, then place it.
   *
   * Done after render rather than from CSS because the direction depends on how
   * much room this particular button has — a row at the bottom of the ebook
   * queue and one at the top need opposite answers, and CSS cannot ask.
   */
  function reposition() {
    if (!trigger || !menuEl) return
    const a = trigger.getBoundingClientRect()
    const m = menuEl.getBoundingClientRect()
    at = placeMenu(
      { top: a.top, left: a.left, width: a.width, height: a.height },
      { width: m.width, height: m.height },
      { width: window.innerWidth, height: window.innerHeight },
    )
  }

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
    trigger?.focus()
  }

  // Click-away and Escape. A menu you cannot dismiss without choosing is a
  // menu that has taken a decision hostage.
  //
  // Scroll and resize reposition rather than close: a `fixed` menu does not
  // travel with the button it belongs to, so left alone it would detach and
  // float over unrelated content.
  $effect(() => {
    if (!open) return
    at = null
    // Measure once the menu is in the DOM. Until `at` is set it renders
    // off-screen (see the style), so nothing flashes in the wrong corner.
    const frame = requestAnimationFrame(async () => {
      reposition()
      // Browsers cannot focus an item while the measured menu is still hidden.
      await tick()
      if (!open || !menuEl?.isConnected) return
      const items = menuEl?.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')
      const current = Array.from(items ?? []).find((item) => item.getAttribute('aria-checked') === 'true')
      ;(current ?? items?.[0])?.focus({ preventScroll: true })
    })
    const away = (e: MouseEvent) => {
      const t = e.target as Node
      if (root && !root.contains(t) && menuEl && !menuEl.contains(t)) open = false
    }
    const key = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        open = false
        trigger?.focus()
      } else if (menuEl?.contains(e.target as Node)) {
        if (e.key === 'Tab') { open = false; trigger?.focus(); return }
        if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return
        const items = Array.from(menuEl.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]'))
        if (!items.length) return
        e.preventDefault()
        const current = items.indexOf(document.activeElement as HTMLButtonElement)
        const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1
          : (current + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
        items[next]?.focus()
      }
    }
    const move = () => reposition()
    document.addEventListener('mousedown', away, true)
    document.addEventListener('keydown', key)
    // `true`: catch scrolling in any ancestor, not just the document.
    window.addEventListener('scroll', move, true)
    window.addEventListener('resize', move)
    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('mousedown', away, true)
      document.removeEventListener('keydown', key)
      window.removeEventListener('scroll', move, true)
      window.removeEventListener('resize', move)
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
      bind:this={trigger}
      {disabled}
      aria-haspopup="menu"
      aria-expanded={open}
      title={current ? detail(current) : ''}
      onclick={() => (open = !open)}
      onkeydown={(event) => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); open = true }
      }}
    >
      {label('agentPicker.by', { name: shortName(current) })}
      <span class="caret" aria-hidden="true">▾</span>
    </button>

    {#if open}
      <!-- `position: fixed`, placed in viewport coordinates. Absolute would be
           clipped by the first scrolling ancestor — the sidecar panel, the
           ebook queue — long before it ever reached a window edge. -->
      <div
        class="menu menu-panel"
        class:up={at?.side === 'up'}
        bind:this={menuEl}
        role="menu"
        aria-label={label('agentPicker.by', { name: shortName(current) })}
        style={at ? `top:${at.top}px; left:${at.left}px` : 'top:0; left:0; visibility:hidden'}
      >
        {#each options as o (o.id)}
          <button
            role="menuitemradio"
            aria-checked={o.id === selected}
            class="item menu-row"
            tabindex="-1"
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
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 5px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--ui-secondary, CanvasText);
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
    position: fixed;
    z-index: 9998;
    min-width: 210px;
    max-width: calc(100vw - 24px);
    max-height: calc(100vh - 24px);
    overflow: auto;
    box-sizing: border-box;
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
  .tick { flex: none; width: 11px; font-size: 11px; line-height: 1.5; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .nm { font-size: 13px; }
  .dt {
    font-size: 12px;
    color: var(--ui-secondary, CanvasText);
    font-family: ui-monospace, SFMono-Regular, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* A harness that cannot run is still selectable — you may be about to fix it
     — but it must not look like a working one. */
  .item.bad .dt { color: var(--ui-danger, #b42318); }
  .menu-panel .menu-row:hover .dt { color: inherit; }
</style>
