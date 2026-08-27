<!-- LibraryPanel.svelte — every book already in the vault, not just what this
     session imported.

     Deliberately NOT merged into the import queue above it: the queue is what
     this window is doing right now (and "Clear finished" empties it), while the
     library is what the vault holds. One list would put a file dropped ten
     seconds ago next to a book imported three months ago, and would make the
     clear button mean two different things.

     All state lives in the parent — this component renders and calls back. The
     reducers behind those callbacks are in lib/library.ts, unit-tested there. -->
<script lang="ts">
  import AgentPicker from '../lib/agent-picker/AgentPicker.svelte'
  import type { AgentOption } from '../lib/agent-picker/types'
  import { formatElapsed } from '../lib/elapsed'
  import { filterBooks, latestSummary, type LibraryBook } from '../lib/library'
  import { t } from '../lib/strings'

  const {
    books,
    agents,
    agentId,
    nowMs,
    onread,
    onopenbook,
    onopensummary,
    onpickagent,
    onrefresh,
  }: {
    books: LibraryBook[]
    agents: AgentOption[]
    agentId: string | null
    nowMs: number
    onread: (book: LibraryBook) => void
    onopenbook: (book: LibraryBook) => void
    onopensummary: (book: LibraryBook) => void
    onpickagent: (id: string) => void
    onrefresh: () => void
  } = $props()

  let query = $state('')
  const shown = $derived(filterBooks(books, query))

  /** The `YYYY-MM-DD` out of a summary path, for the "Digest {date}" badge. */
  function summaryDate(book: LibraryBook): string {
    const file = latestSummary(book)?.split('/').pop() ?? ''
    return file.slice(0, 10)
  }
</script>

<section class="library">
  <div class="head">
    <h2>{t('library.title')}</h2>
    <span class="count">{books.length}</span>
    <input
      type="search"
      bind:value={query}
      placeholder={t('library.search')}
      spellcheck="false"
      autocomplete="off"
    />
    <button class="link" onclick={onrefresh}>{t('library.refresh')}</button>
  </div>

  <div class="rows">
    {#if books.length === 0}
      <p class="empty">{t('library.empty')}</p>
    {:else if shown.length === 0}
      <p class="empty">{t('library.noMatch')}</p>
    {:else}
      {#each shown as b (b.rel)}
        <div class="row">
          <div class="row-main">
            <button class="name" title={b.rel} onclick={() => onopenbook(b)}>{b.name}</button>
            <span class="month">{b.month}</span>

            {#if b.aiStatus === 'queued'}
              <span class="stage">{t('ai.queued')}</span>
            {:else if b.aiStatus === 'running'}
              <span class="stage">
                {t('ai.running', { elapsed: formatElapsed(b.aiStartedAt, nowMs) })}
              </span>
            {:else}
              <!-- A book that has been read shows its digest and offers another
                   read; one that hasn't offers the first. Same pairing with the
                   agent picker as every other surface that starts a run. -->
              {#if latestSummary(b)}
                <button class="link" onclick={() => onopensummary(b)}>
                  {t('library.summaryOn', { date: summaryDate(b) })}
                </button>
                <button class="link" onclick={() => onread(b)}>{t('action.aiReread')}</button>
              {:else}
                <span class="stage unread">{t('library.unread')}</span>
                <button class="link" onclick={() => onread(b)}>{t('action.aiRead')}</button>
              {/if}
              {#if agents.length}
                <AgentPicker
                  options={agents}
                  selected={agentId}
                  onselect={onpickagent}
                  label={t as (k: string, v?: Record<string, string | number>) => string}
                />
              {/if}
            {/if}
          </div>
          {#if b.aiStatus === 'failed' && b.aiError}
            <p class="error">{t('ai.failed')} <span class="detail">{b.aiError}</span></p>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</section>

<style>
  /* Takes whatever height the queue above doesn't, and scrolls its rows
     independently so the search box and count stay put. */
  .library {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-height: 0;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
    padding-top: 10px;
  }
  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h2 {
    font-size: 12px;
    font-weight: 600;
    margin: 0;
  }
  .count {
    font-size: 10px;
    padding: 1px 7px;
    border-radius: 9px;
    background: color-mix(in srgb, currentColor 10%, transparent);
    opacity: 0.7;
  }
  .head input {
    /* An input inherits neither font-size nor family — say both. */
    font: inherit;
    font-size: 12px;
    flex: 1;
    min-width: 0;
    margin-left: auto;
    max-width: 220px;
    padding: 3px 8px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    background: transparent;
    color: inherit;
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button.link {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.65;
    padding: 0;
    flex: none;
  }
  button.link:hover {
    opacity: 1;
  }
  .row {
    padding: 5px 4px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 8%, transparent);
  }
  .row-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  /* The title opens book.md — the book itself is the primary thing here. */
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    text-align: left;
    background: none;
    border: none;
    color: inherit;
    padding: 0;
  }
  .name:hover {
    text-decoration: underline;
  }
  .month,
  .stage {
    font-size: 10px;
    opacity: 0.55;
    flex: none;
  }
  .stage.unread {
    opacity: 0.4;
  }
  .empty {
    opacity: 0.5;
    font-size: 12px;
    text-align: center;
    padding: 14px 0;
  }
  p.error {
    margin: 2px 0 0;
    font-size: 11px;
    color: #c62828;
  }
  .detail {
    display: block;
    margin-top: 2px;
    font-size: 10px;
    opacity: 0.65;
  }
</style>
