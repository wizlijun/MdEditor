<!-- ContextMenu.svelte — the small popup menu the inbox opens on right-click.

     Generic on purpose: it knows about coordinates, focus and dismissal, and
     nothing about ideas. The caller passes the items and gets `onclose` back.

     Reactivity discipline (MEMORY feedback_svelte_effect_untrack): there is not
     a single `$effect` here. Everything that has to happen once — measuring the
     menu to keep it on screen, taking focus, subscribing to the outside click —
     happens in `onMount`, which is not a tracking context, so nothing it writes
     can invalidate itself into the loop that freezes this window.

     Keyboard access, in full:
       * `↑` / `↓`   move focus, skipping disabled items and wrapping around
       * `Home`/`End` jump to the first / last enabled item
       * `Enter` / `Space` trigger the focused item — handled by the browser,
         since each item is a real `<button>`; there is no key handler for them
         precisely so that a `<button>`'s native activation semantics (and the
         `click` event assistive tech synthesizes) are what fires
       * `Esc` closes without choosing, and `Tab` does the same rather than
         letting focus wander out of a menu that is still on screen -->
<script module lang="ts">
  import type { IconName } from '../lib/icons'

  export interface MenuItem {
    label: string
    /** Decorative glyph drawn before the label; the label still carries the
     *  meaning (the icon is `aria-hidden`). Omit it and the row's text lines
     *  up with the others regardless — the icon column is always reserved. */
    icon?: IconName
    /** Runs on activation. Absent (or `disabled`) makes the row inert. */
    onselect?: () => void
    /** Destructive — rendered in the warning color. */
    danger?: boolean
    /** Greyed out and unfocusable; pair it with `title` to say why. */
    disabled?: boolean
    /** Tooltip, and the accessible description of a disabled item. */
    title?: string
    /** Draws a separator line above this item. */
    separated?: boolean
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte'
  import Icon from './Icon.svelte'

  const {
    x,
    y,
    items,
    onclose,
    label,
  }: {
    x: number
    y: number
    items: MenuItem[]
    onclose: () => void
    /** Accessible name of the menu (what it acts on). */
    label: string
  } = $props()

  /** Distance kept from the window edge when the menu has to be flipped. */
  const MARGIN = 6

  let el: HTMLDivElement | undefined = $state()
  /**
   * Starts at the pointer and is corrected once, after measuring (see
   * `onMount`). Capturing `x`/`y` at construction is deliberate — a menu does
   * not follow the pointer around after it opens, and the caller wraps this
   * component in `{#key}` so that opening it somewhere else builds a new one.
   */
  // svelte-ignore state_referenced_locally
  let pos = $state({ left: x, top: y })

  function enabledButtons(): HTMLButtonElement[] {
    return el ? Array.from(el.querySelectorAll<HTMLButtonElement>('button:not([disabled])')) : []
  }

  function move(delta: number): void {
    const buttons = enabledButtons()
    if (buttons.length === 0) return
    const at = buttons.findIndex((b) => b === document.activeElement)
    // A menu is a ring: past the end is the start again. `at === -1` (focus is
    // on the container itself, right after opening) enters from whichever end
    // the key implies.
    const next = at === -1 ? (delta > 0 ? 0 : buttons.length - 1) : (at + delta + buttons.length) % buttons.length
    buttons[next].focus()
  }

  function onkeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case 'Escape':
        e.preventDefault()
        e.stopPropagation()
        onclose()
        break
      case 'ArrowDown':
        e.preventDefault()
        move(1)
        break
      case 'ArrowUp':
        e.preventDefault()
        move(-1)
        break
      case 'Home':
        e.preventDefault()
        enabledButtons()[0]?.focus()
        break
      case 'End': {
        e.preventDefault()
        const buttons = enabledButtons()
        buttons[buttons.length - 1]?.focus()
        break
      }
      case 'Tab':
        // Focus must not leave an open menu; closing is the honest response.
        e.preventDefault()
        onclose()
        break
    }
  }

  function choose(item: MenuItem): void {
    if (item.disabled) return
    // Close first: the action may open a dialog or move focus, and a menu still
    // sitting on top of it would swallow the click that follows.
    onclose()
    item.onselect?.()
  }

  onMount(() => {
    // Flip rather than clamp: a menu opened near the right/bottom edge grows
    // toward the pointer's other side, so the pointer never lands on top of an
    // item and the whole menu stays visible.
    if (el) {
      const { width, height } = el.getBoundingClientRect()
      const left = x + width > window.innerWidth - MARGIN ? Math.max(MARGIN, x - width) : x
      const top = y + height > window.innerHeight - MARGIN ? Math.max(MARGIN, y - height) : y
      pos = { left, top }
    }
    // Focus the menu itself, not its first item: opening with a *pointer*
    // should not pre-select a destructive row, while `↓` still enters the list
    // (see `move`) and `Esc` works immediately because the container has focus.
    el?.focus()

    // Capture phase, on `mousedown` rather than `click`: the press is what
    // dismisses a menu, and capturing means a press on some other control
    // closes this first instead of being eaten by it. A right-click on another
    // row also lands here first (mousedown precedes contextmenu), so the caller
    // reopens the menu at the new position instead of stacking two.
    const onOutside = (e: MouseEvent) => {
      if (el && e.target instanceof Node && el.contains(e.target)) return
      onclose()
    }
    window.addEventListener('mousedown', onOutside, true)
    return () => window.removeEventListener('mousedown', onOutside, true)
  })
</script>

<div
  bind:this={el}
  class="menu"
  role="menu"
  aria-label={label}
  aria-orientation="vertical"
  tabindex="-1"
  style="left:{pos.left}px; top:{pos.top}px"
  {onkeydown}
>
  {#each items as item, i (i)}
    {#if item.separated}
      <div class="sep" role="separator"></div>
    {/if}
    <button
      type="button"
      role="menuitem"
      tabindex="-1"
      class:danger={item.danger}
      disabled={item.disabled}
      title={item.title}
      onclick={() => choose(item)}
    >
      <!-- Rendered whether or not the item has an icon — see `.glyph` in the
           styles for why the column is reserved. Hidden from assistive tech:
           the label beside it already says what the row does. -->
      <span class="glyph" aria-hidden="true">
        {#if item.icon}<Icon name={item.icon} />{/if}
      </span>
      {item.label}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 30;
    min-width: 180px;
    padding: 4px;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  .menu:focus { outline: none; }
  button {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    width: 100%;
    padding: 0.32rem 0.6rem;
    border: 0;
    border-radius: 5px;
    background: none;
    color: inherit;
    /* button inherits neither font-size nor family — see MEMORY
       reference_button_no_inherit_font. */
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: color-mix(in srgb, currentColor 12%, transparent);
    outline: none;
  }
  button:disabled { opacity: 0.45; cursor: default; }
  /* Fixed 16px column, held open even for an item without an icon. Every item
     this window builds today does have one, so nothing currently depends on
     it; the column is reserved because this component is generic (it knows
     about coordinates, focus and dismissal, not about ideas) and must not
     assume its caller supplies an icon for every row — a mixed menu would
     otherwise have its labels stepping in and out. */
  .glyph {
    flex: 0 0 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 16px;
  }
  .danger { color: #dc2626; }
  .danger:hover:not(:disabled),
  .danger:focus-visible { background: color-mix(in srgb, #dc2626 14%, transparent); }
  .sep {
    height: 1px;
    margin: 4px 2px;
    background: var(--line, #e5e7eb);
  }
</style>
