<script lang="ts">
  import type { RunRecord } from '../lib/events'
  import { fmtShort } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'

  /** This plugin's own id; a run stamped with any other harness is labelled. */
  const SELF_ID = 'notemd.claude-agent'
  /** `notemd.claude-agent` → `claude` — enough to tell them apart in a row. */
  const shortHarness = (id: string) => id.replace(/^notemd\./, '').replace(/-agent$/, '')

  let { runs, label, empty, showTask = false, selectedId = null, onselect, ondelete, onclear }:
    {
      runs: RunRecord[]
      label: (k: MessageKey, v?: Record<string, string | number>) => string
      empty: string
      /** In the all-tasks view each row needs to say WHICH task it was. */
      showTask?: boolean
      selectedId?: string | null
      onselect: (run: RunRecord) => void
      ondelete: (run: RunRecord) => void
      onclear: () => void
    } = $props()

  // Right-click target + where to draw the menu.
  let menu: { run: RunRecord; x: number; y: number } | null = $state(null)

  function openMenu(e: MouseEvent, run: RunRecord) {
    e.preventDefault()
    menu = { run, x: e.clientX, y: e.clientY }
  }
  function closeMenu() {
    menu = null
  }

  // "2026-07-31T00:42:33Z" (UTC) → "07-31 08:42" in the user's local timezone
  const when = fmtShort
</script>

<svelte:window
  onmousedown={(e) => {
    if (menu && !(e.target as HTMLElement | null)?.closest('.ctx')) closeMenu()
  }}
  onkeydown={(e) => {
    if (menu && e.key === 'Escape') closeMenu()
  }}
/>

{#if runs.length === 0}
  <p class="empty">{empty}</p>
{:else}
  <ul class="history">
    {#each runs as run (run.run_id)}
      <li>
        <button
          class="row"
          class:active={run.run_id === selectedId}
          aria-pressed={run.run_id === selectedId}
          title={run.result || run.stderr_tail}
          onclick={() => onselect(run)}
          oncontextmenu={(e) => openMenu(e, run)}
        >
          <span class="row-top">
            <span class="status s-{run.status}">{label(('status.' + run.status) as MessageKey)}</span>
            {#if showTask}<span class="task">{run.task}</span>{/if}
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
{/if}

{#if menu}
  <div class="ctx menu-panel" style="left: {menu.x}px; top: {menu.y}px" role="menu" tabindex="-1">
    <button
      class="menu-row"
      role="menuitem"
      onclick={() => {
        const r = menu!.run
        closeMenu()
        ondelete(r)
      }}>{label('history.delete')}</button
    >
    <button
      role="menuitem"
      class="menu-row danger"
      onclick={() => {
        closeMenu()
        onclear()
      }}>{label('history.clearAll')}</button
    >
  </div>
{/if}

<style>
  .history {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 11px;
  }
  .history li + li { margin-top: 4px; }
  .row {
    font: inherit;
    font-size: 11px;
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
    background: color-mix(in srgb, var(--standard-accent, #3479db) 8%, Canvas);
    box-shadow: inset 3px 0 0 var(--standard-accent, #3479db);
  }
  .row:focus-visible { outline: 2px solid var(--standard-accent, #3479db); outline-offset: 2px; }
  .row-top, .row-meta { display: flex; align-items: center; min-width: 0; }
  .row-top { gap: 6px; }
  .row-meta { gap: 5px; margin-top: 4px; color: var(--muted-text, currentColor); font-size: 9.5px; }
  .status { font-weight: 600; flex: none; }
  .s-error, .s-timeout { color: #d9534f; }
  .s-skipped { opacity: 0.65; }
  .task {
    opacity: 0.75;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when { flex: none; font-variant-numeric: tabular-nums; }
  .other {
    flex: none;
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
  .empty { font-size: 11px; opacity: 0.55; margin: 4px 0; }
  .ctx {
    position: fixed;
    z-index: 50;
    min-width: 140px;
    padding: 4px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
    background: Canvas;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.22);
  }
  .ctx button {
    font: inherit;
    font-size: 12px;
    display: block;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .ctx button:hover { background: color-mix(in srgb, currentColor 12%, transparent); }
  .ctx .danger { color: #d24b4b; }
</style>
