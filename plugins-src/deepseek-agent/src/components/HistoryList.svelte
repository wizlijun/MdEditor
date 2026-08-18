<script lang="ts">
  import type { RunRecord } from '../lib/events'
  import { fmtShort } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'

  /** This plugin's own id; a run stamped with any other harness is labelled. */
  const SELF_ID = 'notemd.deepseek-agent'
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
          title={run.result || run.stderr_tail}
          onclick={() => onselect(run)}
          oncontextmenu={(e) => openMenu(e, run)}
        >
          <span class="status s-{run.status}">{label(('status.' + run.status) as MessageKey)}</span>
          {#if showTask}<span class="task">{run.task}</span>{/if}
          <span class="when">{when(run.started_at)}</span>
          {#if run.trigger === 'cli'}<span class="cli">CLI</span>{/if}
          <!-- Both agent plugins share one runs root, so this list shows the
               OTHER harness's runs too. Unlabelled, a claude failure here reads
               as a deepseek failure — which is exactly how an expired Claude
               credential got blamed on DeepSeek. -->
          {#if run.harness && run.harness !== SELF_ID}
            <span class="other" title={run.harness}>{shortHarness(run.harness)}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
{/if}

{#if menu}
  <div class="ctx" style="left: {menu.x}px; top: {menu.y}px" role="menu" tabindex="-1">
    <button
      role="menuitem"
      onclick={() => {
        const r = menu!.run
        closeMenu()
        ondelete(r)
      }}>{label('history.delete')}</button
    >
    <button
      role="menuitem"
      class="danger"
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
  /* Buttons inherit neither font-size nor family — say both, or rows drift. */
  .row {
    font: inherit;
    font-size: 11px;
    display: flex;
    gap: 6px;
    align-items: baseline;
    width: 100%;
    padding: 3px 5px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover { background: color-mix(in srgb, currentColor 8%, transparent); }
  .row.active { background: color-mix(in srgb, currentColor 15%, transparent); }
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
  .when { opacity: 0.55; margin-left: auto; flex: none; font-variant-numeric: tabular-nums; }
  /* A run from the other agent: present, readable, and clearly not ours. */
  .other {
    flex: none;
    font-size: 9px;
    padding: 0 4px;
    border-radius: 3px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    opacity: 0.65;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .cli {
    font-size: 9px;
    opacity: 0.5;
    flex: none;
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 3px;
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
