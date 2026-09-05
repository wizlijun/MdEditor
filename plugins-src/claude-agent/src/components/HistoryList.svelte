<script lang="ts">
  import { tick } from 'svelte'
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import type { RunRecord } from '../lib/events'
  import { fmtShort } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'
  import { HISTORY_PAGE_SIZE, nextHistoryBatchSize, nextHistoryCount } from '../lib/history-pagination'

  /** This plugin's own id; a run stamped with any other harness is labelled. */
  const SELF_ID = 'notemd.claude-agent'
  /** `notemd.claude-agent` → `claude` — enough to tell them apart in a row. */
  const shortHarness = (id: string) => id.replace(/^notemd\./, '').replace(/-agent$/, '')

  let { runs, label, empty, selectedId = null, onselect, ondelete, onclear }:
    {
      runs: RunRecord[]
      label: (k: MessageKey, v?: Record<string, string | number>) => string
      empty: string
      selectedId?: string | null
      onselect: (run: RunRecord) => void
      ondelete: (run: RunRecord) => void | Promise<void>
      onclear: () => void | Promise<void>
    } = $props()

  let visibleCount = $state(HISTORY_PAGE_SIZE)
  let historyList: HTMLUListElement | undefined = $state()
  let emptyState: HTMLParagraphElement | undefined = $state()
  const visibleRuns = $derived(runs.slice(0, visibleCount))
  const moreCount = $derived(nextHistoryBatchSize(visibleCount, runs.length))

  async function showMore() {
    const firstNew = visibleCount
    visibleCount = nextHistoryCount(visibleCount, runs.length)
    await tick()
    if (moreCount === 0) historyList?.querySelectorAll<HTMLButtonElement>('.row')[firstNew]?.focus()
  }

  // Right-click target + where to draw the menu.
  let menu: { run: RunRecord; x: number; y: number } | null = $state(null)

  let menuElement: HTMLDivElement | undefined = $state()
  let menuTrigger: HTMLElement | null = null
  let confirmation: { run: RunRecord | null } | null = $state(null)
  let deleting = $state(false)

  async function openMenu(e: MouseEvent | KeyboardEvent, run: RunRecord) {
    e.preventDefault()
    menuTrigger = e.currentTarget as HTMLElement
    const rect = menuTrigger.getBoundingClientRect()
    menu = { run, x: e instanceof MouseEvent ? e.clientX : rect.left, y: e instanceof MouseEvent ? e.clientY : rect.bottom }
    await tick()
    if (!menu || !menuElement) return
    menu = { ...menu, x: Math.max(8, Math.min(menu.x, window.innerWidth - menuElement.offsetWidth - 8)), y: Math.max(8, Math.min(menu.y, window.innerHeight - menuElement.offsetHeight - 8)) }
    menuElement.querySelector<HTMLButtonElement>('button')?.focus()
  }
  function closeMenu(restore = true) {
    menu = null
    if (restore) menuTrigger?.focus()
  }
  function menuKeydown(e: KeyboardEvent) {
    const buttons = Array.from(menuElement?.querySelectorAll<HTMLButtonElement>('button') ?? [])
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
      e.preventDefault()
      const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (index + (e.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
      buttons[next]?.focus()
    } else if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault()
      e.stopPropagation()
      closeMenu()
    }
  }
  function askDelete(run: RunRecord | null) {
    closeMenu()
    confirmation = { run }
  }
  async function confirmDelete() {
    if (!confirmation || deleting) return
    deleting = true
    try {
      if (confirmation.run) await ondelete(confirmation.run)
      else await onclear()
    } finally {
      deleting = false
      confirmation = null
      await tick()
      // The deleted trigger may no longer exist when modal focus returns.
      if (!menuTrigger?.isConnected) (historyList?.querySelector<HTMLButtonElement>('.row') ?? emptyState)?.focus()
    }
  }

  // "2026-07-31T00:42:33Z" (UTC) → "07-31 08:42" in the user's local timezone
  const when = fmtShort
</script>

<svelte:window
  onmousedown={(e) => {
    if (menu && !(e.target as HTMLElement | null)?.closest('.ctx')) closeMenu(false)
  }}
  onkeydown={(e) => {
    if (menu && e.key === 'Escape') closeMenu()
  }}
/>

{#if runs.length === 0}
  <p class="empty" tabindex="-1" bind:this={emptyState}>{empty}</p>
{:else}
  <ul class="history" id="recent-runs" bind:this={historyList}>
    {#each visibleRuns as run (run.run_id)}
      <li>
        <button
          class="row"
          class:active={run.run_id === selectedId}
          aria-current={run.run_id === selectedId ? 'page' : undefined}
          title={run.result || run.stderr_tail}
          onclick={() => onselect(run)}
          oncontextmenu={(e) => openMenu(e, run)}
          onkeydown={(e) => { if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) void openMenu(e, run) }}
        >
          <span class="row-top">
            <span class="status s-{run.status}">{label(('status.' + run.status) as MessageKey)}</span>
            <span class="task">{run.task}</span>
          </span>
          <span class="row-meta">
            <span class="when">{when(run.started_at)}</span>
            {#if run.trigger === 'cli'}<span class="cli">CLI</span>{/if}
            <!-- Runs from another provider stay visible, but clearly labelled. -->
            {#if run.harness && run.harness !== SELF_ID}
              <span class="other" title={run.harness}>{shortHarness(run.harness)}</span>
            {/if}
          </span>
        </button>
      </li>
    {/each}
  </ul>
  {#if moreCount > 0}
    <button class="more" type="button" aria-controls="recent-runs" onclick={showMore}>
      {label('history.more', { n: moreCount })}
    </button>
  {/if}
{/if}

{#if menu}
  <div class="ctx menu-panel" bind:this={menuElement} style="left: {menu.x}px; top: {menu.y}px" role="menu" aria-label={label('history.actions')} tabindex="-1" onkeydown={menuKeydown}>
    <button class="menu-row" role="menuitem" onclick={() => askDelete(menu!.run)}>{label('history.delete')}</button>
    <button class="menu-row danger" role="menuitem" onclick={() => askDelete(null)}>{label('history.clearAll')}</button>
  </div>
{/if}

{#if confirmation}
  <div class="confirm-overlay">
    <div class="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="history-confirm-title" aria-describedby="history-confirm-hint" tabindex="-1" use:modalFocus={{ onClose: () => { confirmation = null }, canClose: () => !deleting }}>
      <h2 id="history-confirm-title">{label(confirmation.run ? 'history.confirmDelete' : 'history.confirmClear')}</h2>
      {#if confirmation.run}<p class="target">{confirmation.run.task}</p>{/if}
      <p id="history-confirm-hint">{label('history.confirmHint')}</p>
      <div class="confirm-actions">
        <button type="button" data-initial-focus disabled={deleting} onclick={() => { confirmation = null }}>{label('history.cancel')}</button>
        <button type="button" class="danger" disabled={deleting} onclick={confirmDelete}>{label(deleting ? 'history.deleting' : confirmation.run ? 'history.delete' : 'history.clearAll')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .history {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 12px;
  }
  .history li + li { margin-top: 4px; }
  .row {
    font: inherit;
    font-size: 12px;
    display: block;
    width: 100%;
    padding: 8px 9px 7px;
    border: 1px solid var(--window-border, color-mix(in srgb, currentColor 11%, transparent));
    border-radius: 10px;
    background: var(--card-surface, transparent);
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    border-color: var(--strong-border, color-mix(in srgb, currentColor 18%, transparent));
    background: var(--hover-surface, color-mix(in srgb, currentColor 5%, transparent));
  }
  .row.active {
    border-color: color-mix(in srgb, var(--standard-accent, #3479db) 45%, transparent);
    background: var(--ui-selection);
    box-shadow: inset 3px 0 0 var(--standard-accent, #3479db);
  }
  .row:focus-visible { outline: 2px solid var(--standard-accent, #3479db); outline-offset: 2px; }
  .row-top, .row-meta { display: flex; align-items: center; min-width: 0; }
  .row-top { gap: 6px; }
  .row-meta { flex-wrap: wrap; gap: 5px; margin-top: 4px; color: var(--muted-text, currentColor); font-size: 12px; }
  .status { font-weight: 600; flex: none; }
  .s-error, .s-timeout { color: var(--ui-danger); }
  .s-skipped { color: var(--ui-secondary); }
  .task {
    color: var(--ui-secondary);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when { flex: none; font-variant-numeric: tabular-nums; }
  .other {
    flex: 0 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
    padding: 1px 5px;
    border-radius: 999px;
    border: 1px solid var(--window-border, color-mix(in srgb, currentColor 15%, transparent));
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .cli {
    flex: none;
    border-radius: 999px;
    padding: 1px 5px;
    background: color-mix(in srgb, currentColor 7%, transparent);
  }
  .empty { font-size: 12px; color: var(--ui-secondary); margin: 4px 0; }
  .more {
    width: 100%; margin-top: 6px; padding: 7px 9px; border: 1px solid transparent;
    border-radius: 9px; background: transparent; color: var(--muted-text, currentColor);
    font: inherit; font-size: 12px; cursor: pointer;
  }
  .more:hover { border-color: var(--window-border); background: var(--hover-surface); color: CanvasText; }
  .more:focus-visible { outline: 2px solid var(--standard-accent, #3479db); outline-offset: 2px; }
  .ctx { position: fixed; z-index: 50; min-width: 160px; max-width: calc(100vw - 16px); }
  .ctx button { font: inherit; width: 100%; border: 0; background: none; color: inherit; text-align: left; }
  .ctx .danger:not(:hover) { color: var(--ui-danger); }
  .confirm-overlay { position: fixed; inset: 0; z-index: 60; display: grid; place-items: center; padding: 16px; background: rgb(0 0 0 / 0.35); }
  .confirm-dialog { width: min(380px, 100%); max-height: calc(100dvh - 32px); overflow: auto; box-sizing: border-box; padding: 20px; border: 1px solid var(--ui-separator); border-radius: 14px; background: var(--ui-surface); box-shadow: 0 16px 48px rgb(0 0 0 / 0.2); }
  .confirm-dialog h2 { margin: 0 0 10px; font-size: 17px; }
  .confirm-dialog p { color: var(--ui-secondary); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
  .confirm-dialog .target { color: CanvasText; font-weight: 600; }
  .confirm-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-top: 18px; }
  .confirm-actions button { font: inherit; padding: 6px 12px; min-height: 32px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: CanvasText; }
  .confirm-actions button:hover { background: var(--ui-hover); }
  .confirm-actions .danger { color: var(--ui-danger); }

</style>
