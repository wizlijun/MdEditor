<script lang="ts">
  import { onDestroy } from 'svelte'
  import { listen } from '@tauri-apps/api/event'
  import type { Tab } from '../../lib/tabs.svelte'
  import { openFile } from '../../lib/tabs.svelte'
  import { requestReveal } from '../../lib/outline/reveal.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import { showError } from '../../lib/dialogs'
  import { setSideVisible } from '../../lib/side-panel/registry.svelte'
  import SideViewSwitcher from './SideViewSwitcher.svelte'
  import { searchStore, isIndexNotReady } from '../../lib/search/store.svelte'
  import { searchApi, type SearchHit } from '../../lib/search/api'

  // `tab` is part of every side view's props contract (see SidePanel.svelte);
  // this panel is vault-wide rather than per-document, so only the
  // `SideViewSwitcher` below reads it.
  let { tab }: { tab: Tab | null } = $props()

  // Local input value — kept separate from `searchStore.query` so keystrokes
  // are never lost to the 200ms debounce window (typing updates this
  // immediately; the store lags behind on purpose).
  let inputValue = $state('')
  let debounceTimer: ReturnType<typeof setTimeout> | undefined
  onDestroy(() => { if (debounceTimer) clearTimeout(debounceTimer) })

  // `notemd_search_rebuild` holds the index lock for its whole duration, so a
  // query fired mid-rebuild would just hang with no visible cause. Disabling
  // the input for that window turns a silent hang into an honest wait state.
  let rebuilding = $state(false)

  // Anything other than the known "not ready" case is shown as-is — better
  // than nothing, not pretending to translate arbitrary Rust error text.
  let errorText = $derived(
    isIndexNotReady(searchStore.error) ? t('search.notReady') : searchStore.error,
  )

  function scheduleSearch() {
    if (debounceTimer) clearTimeout(debounceTimer)
    debounceTimer = setTimeout(() => { void searchStore.run(inputValue) }, 200)
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      if (debounceTimer) clearTimeout(debounceTimer)
      void searchStore.run(inputValue)
    } else if (e.key === 'Escape') {
      if (debounceTimer) clearTimeout(debounceTimer)
      inputValue = ''
      searchStore.clear()
    }
  }

  async function onRebuild() {
    rebuilding = true
    try {
      await searchApi.rebuild()
      if (inputValue.trim()) await searchStore.run(inputValue)
    } catch (e) {
      showError(String(e))
    } finally {
      rebuilding = false
    }
  }

  async function onOpenHit(hit: SearchHit) {
    try {
      await openFile(hit.absPath)
      // `text` is a whole block and may span several source lines; the reveal
      // anchor only ever matches within a single rendered line/text node, so
      // hand it the first non-blank line rather than the raw multi-line block.
      const anchor = hit.text.split('\n').find((l) => l.trim().length > 0)?.trim() ?? hit.text
      requestReveal(hit.line, anchor)
    } catch (e) {
      showError(String(e))
    }
  }

  // Reindex on save elsewhere in the vault; if a query is active, silently
  // refresh it so newly-searchable content shows up without the user
  // re-typing. Skip while a request is already in flight so a burst of
  // index-updated events (e.g. several saves in a row) can't stack multiple
  // overlapping run()s on top of each other — the sequence guard in the
  // store would just discard all but the last anyway, so this is purely to
  // avoid pointless calls, not correctness.
  //
  // KNOWN GAP: this does not protect against a *rebuild* triggered
  // elsewhere (another window, a concurrent CLI invocation). If this event
  // fires while some other actor holds the backend index mutex mid-rebuild,
  // the rerun below still blocks for that rebuild's full duration with only
  // the generic `loading` state to show for it — the same class of silent
  // hang the brief flagged for this panel's own rebuild button, just
  // triggered externally instead of from this UI. A real fix needs a
  // backend-reported "busy" signal (e.g. a distinct event/state emitted
  // around `notemd_search_rebuild`), which is out of scope for this panel.
  $effect(() => {
    const pending = listen('search://index-updated', () => {
      if (searchStore.query.trim() && !searchStore.loading) void searchStore.run(searchStore.query)
    })
    return () => { void pending.then((un) => un()) }
  })
</script>

<div class="search-content">
  <header>
    <button class="hbtn" title={t('search.hide')} aria-label={t('search.hide')} onclick={() => void setSideVisible('left', false)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2" />
        <line x1="9" y1="3" x2="9" y2="21" />
        <polyline points="16 15 13 12 16 9" />
      </svg>
    </button>
    <SideViewSwitcher side="left" {tab} />
  </header>

  <div class="search-input-row">
    <input
      type="text"
      class="search-input"
      placeholder={t('search.placeholder')}
      bind:value={inputValue}
      oninput={scheduleSearch}
      onkeydown={onKeydown}
      disabled={rebuilding}
    />
  </div>

  <div class="body">
    {#if searchStore.error}
      <div class="error-row">
        <p class="error">{errorText}</p>
        <button class="rebuild-btn" onclick={() => void onRebuild()} disabled={rebuilding}>
          {t('search.rebuild')}
        </button>
      </div>
    {:else if rebuilding}
      <p class="empty">{t('search.notReady')}</p>
    {:else if searchStore.loading}
      <p class="empty">…</p>
    {:else if searchStore.route !== null && searchStore.hits.length === 0}
      <p class="empty">{t('search.noResults')}</p>
    {:else if searchStore.hits.length > 0}
      <ul class="hits">
        {#each searchStore.hits as hit, i (hit.path + ':' + hit.line + ':' + i)}
          <li class="hit">
            <button class="row" onclick={() => void onOpenHit(hit)}>
              <span class="breadcrumb" title={hit.breadcrumb}>{hit.breadcrumb}</span>
              <span class="text">
                {#if hit.agentBy}
                  <span class="marker" title={t('search.agentWritten', { agent: hit.agentBy })}>✦</span>
                {/if}
                {#if hit.humanVerified}
                  <span class="marker" title={t('search.humanVerified')}>●</span>
                {/if}
                {hit.text}
              </span>
              <span class="loc">{hit.path}:{hit.line}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if !searchStore.error}
    <footer class="status">
      {#if searchStore.route !== null}
        <span>{t('search.resultCount', { n: searchStore.total, ms: searchStore.tookMs })}</span>
        {#if searchStore.route === 't1-scan'}
          <span class="fallback">{t('search.fallbackScan')}</span>
        {/if}
      {/if}
    </footer>
  {/if}
</div>

<style>
  .search-content {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--drawer-bg, #f6f6f6);
  }
  header {
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid var(--border-color, #3333);
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .hbtn {
    display: inline-flex; align-items: center; justify-content: center;
    border: 0; background: transparent; cursor: pointer;
    padding: 3px; border-radius: 4px; opacity: 0.7;
  }
  .hbtn svg { display: block; }
  .hbtn:hover:not(:disabled) { background: rgba(0,0,0,0.08); opacity: 1; }
  .search-input-row {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border-color, #3333);
  }
  .search-input {
    width: 100%; box-sizing: border-box;
    border: 1px solid rgba(0,0,0,0.15); border-radius: 5px;
    padding: 4px 8px; font: inherit; font-size: 12px;
    background: var(--input-bg, #fff); color: inherit;
  }
  .search-input:focus { outline: none; border-color: rgba(0,120,255,0.6); }
  .search-input:disabled { opacity: 0.6; }
  .body { flex: 1; overflow-y: auto; padding: 4px; }
  .empty { padding: 8px; opacity: 0.5; font-size: 12px; }
  .error-row { padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .error { margin: 0; font-size: 12px; color: #c0392b; }
  .rebuild-btn {
    align-self: flex-start;
    font-size: 12px; padding: 3px 8px; border-radius: 4px;
    border: 1px solid var(--border-color, #3335); background: transparent; cursor: pointer;
  }
  .rebuild-btn:hover:not(:disabled) { background: rgba(0,0,0,0.06); }
  .rebuild-btn:disabled { opacity: 0.5; cursor: default; }
  .hits { list-style: none; margin: 0; padding: 0; }
  .hit { border-radius: 6px; }
  .row {
    display: flex; flex-direction: column; gap: 2px;
    width: 100%; text-align: left;
    border: 0; background: transparent; cursor: pointer;
    padding: 6px 8px; border-radius: 6px;
  }
  .row:hover { background: rgba(0,0,0,0.05); }
  .breadcrumb {
    font-size: 11px; opacity: 0.55;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .text { font-size: 13px; }
  .marker { opacity: 0.7; margin-right: 3px; }
  .loc { font-size: 11px; opacity: 0.45; }
  .status {
    padding: 5px 10px; font-size: 11px; opacity: 0.6;
    border-top: 1px solid var(--border-color, #3333);
    display: flex; gap: 8px; align-items: center;
    flex-wrap: wrap;
  }
  .fallback { opacity: 0.8; }
  @media (prefers-color-scheme: dark) {
    .search-content { background: var(--drawer-bg, #1c1c1e); }
    .hbtn:hover:not(:disabled) { background: rgba(255,255,255,0.1); }
    .search-input { border-color: rgba(255,255,255,0.18); background: var(--input-bg, #2a2a2c); }
    .row:hover { background: rgba(255,255,255,0.08); }
    .rebuild-btn:hover:not(:disabled) { background: rgba(255,255,255,0.1); }
    .error { color: #ff6b5e; }
  }
</style>
