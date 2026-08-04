<!-- HistoryList.svelte — every idea in the idea directory, newest first.
     A row is a button: clicking it loads that idea into the editor. Rows whose
     proof document exists get a second button that opens the result in the main
     window's editor. Status comes from `statusOf` (proof file > active run >
     last failure > draft) — the list never stores a status of its own. -->
<script lang="ts">
  import { displayName, openResult, state as store, statusOf } from '../lib/store.svelte'
  import { t, type MessageKey } from '../lib/strings'
  import type { IdeaStatus } from '../lib/status'

  const { onselect }: { onselect: (name: string) => void } = $props()

  const STATUS_KEY: Record<IdeaStatus, MessageKey> = {
    draft: 'statusDraft',
    running: 'statusRunning',
    done: 'statusDone',
    failed: 'statusFailed',
  }
</script>

<aside class="history">
  <h2>{t('history')}</h2>
  <ul>
    {#each store.docs as name (name)}
      {@const status = statusOf(store, name)}
      <li class:current={name === store.current}>
        <button class="row" type="button" onclick={() => onselect(name)} title={name}>
          <span class="name">{displayName(name)}</span>
          <span class="badge {status}">{t(STATUS_KEY[status])}</span>
        </button>
        {#if status === 'done'}
          <button class="open" type="button" onclick={() => openResult(name)}>
            {t('openResult')}
          </button>
        {/if}
      </li>
    {/each}
  </ul>
</aside>

<style>
  .history {
    width: 240px;
    flex: 0 0 240px;
    box-sizing: border-box;
    padding: 0.75rem;
    border-left: 1px solid var(--line, #e5e7eb);
    overflow-y: auto;
  }
  h2 {
    margin: 0 0 0.6rem;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.6;
  }
  ul { list-style: none; margin: 0; padding: 0; }
  li {
    display: flex;
    flex-direction: column;
    border-radius: 6px;
    margin-bottom: 2px;
  }
  li.current { background: color-mix(in srgb, currentColor 8%, transparent); }
  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.4rem 0.45rem;
    background: none;
    border: 0;
    border-radius: 6px;
    color: inherit;
    /* button does NOT inherit font-size/family — see MEMORY
       reference_button_no_inherit_font; both must be stated explicitly. */
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
  }
  .row:hover { background: color-mix(in srgb, currentColor 10%, transparent); }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    flex: 0 0 auto;
    font-size: 0.68rem;
    padding: 1px 6px;
    border-radius: 999px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    opacity: 0.8;
  }
  .badge.running { background: color-mix(in srgb, var(--accent, #2563eb) 22%, transparent); opacity: 1; }
  .badge.done { background: color-mix(in srgb, #16a34a 25%, transparent); opacity: 1; }
  .badge.failed { background: color-mix(in srgb, #dc2626 25%, transparent); opacity: 1; }
  .open {
    align-self: flex-start;
    margin: 0 0 0.35rem 0.45rem;
    padding: 0;
    background: none;
    border: 0;
    color: var(--accent, #2563eb);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .open:hover { text-decoration: underline; }
</style>
