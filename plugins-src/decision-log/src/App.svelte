<!-- App.svelte — Decision window shell. On mount runs refresh() to load the
     board / candidates / archives / scoreboard, then renders the three-column
     Board plus the always-on Scoreboard rail.
     Declares `color-scheme: light dark` so this standalone plugin window follows
     the system appearance (project convention — otherwise Canvas system colors
     get pinned to light; see MEMORY reference_webview_color_scheme). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { state as store, refresh } from './lib/store.svelte'
  import Board from './components/Board.svelte'
  import Scoreboard from './components/Scoreboard.svelte'
  import { bridge } from './lib/bridge'
  import { setLocale, t } from './lib/strings'

  setLocale(bridge().locale)

  onMount(async () => {
    try {
      await refresh()
    } catch (e) {
      console.error('[decision-log] refresh failed:', e)
      store.loading = false
    }
  })
</script>

<main class="app">
  <header class="topbar">
    <h1>{t('panel.title')}</h1>
    <p class="value-line">{t('value.line')}</p>
  </header>

  {#if store.loading}
    <div class="loading">{t('common.loading')}</div>
  {:else}
    <div class="content">
      <Board />
      <Scoreboard />
    </div>
  {/if}
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
  }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
  }
  .topbar {
    flex: 0 0 auto;
    display: flex;
    align-items: baseline;
    gap: 0.9rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--line, #e5e7eb);
  }
  .topbar h1 { margin: 0; font-size: 1.05rem; flex: 0 0 auto; }
  /* R1: one-line value proposition — always visible, quiet styling. */
  .value-line {
    margin: 0; font-size: 0.82rem; opacity: 0.6; line-height: 1.4;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0;
  }
  .content {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .loading {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.6;
  }
</style>
