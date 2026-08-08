<script lang="ts">
  import { dispatch } from '../lib/commands'
  import { getRecentFiles } from '../lib/settings.svelte'
  import { mergedRecents } from '../lib/recent-sync.svelte'
  import { openFile } from '../lib/tabs.svelte'
  import { showError } from '../lib/dialogs'
  import VaultBrowser from './VaultBrowser.svelte'
  import { t } from '../lib/i18n/store.svelte'
  import { basename } from '../lib/paths'

  let { open = $bindable(false) }: { open?: boolean } = $props()
  let recents = $derived(mergedRecents.paths.length ? mergedRecents.paths : getRecentFiles())

  async function pickRecent(p: string) {
    open = false
    try { await openFile(p) } catch (e) { console.warn(e); showError(String(e)) }
  }

</script>

<aside class:open class="drawer">
  <div class="head">note.md</div>
  <button class="row primary" onclick={() => { open = false; dispatch('open') }}>📂 Open File</button>
  <VaultBrowser onCloseDrawer={() => (open = false)} />
  <div class="section-label">Recent</div>
  {#if recents.length === 0}
    <div class="empty">No recent files</div>
  {:else}
    {#each recents as r (r)}
      <button class="row" onclick={() => pickRecent(r)}>{basename(r)}</button>
    {/each}
  {/if}
  <div class="spacer"></div>
  <button class="row" onclick={() => { open = false; dispatch('preferences') }}>⚙️ Settings</button>
</aside>

{#if open}
  <button class="overlay" aria-label={t('drawer.closeMenu')} onclick={() => (open = false)}></button>
{/if}

<style>
  .drawer {
    position: fixed; top: 0; left: -320px; height: 100%;
    width: min(85vw, 320px);
    background: var(--drawer-bg, white);
    box-shadow: 2px 0 12px rgba(0,0,0,0.08);
    transition: left 0.2s ease;
    z-index: 60;
    display: flex; flex-direction: column;
    padding: env(safe-area-inset-top, 0) 0 env(safe-area-inset-bottom, 0) 0;
  }
  .drawer.open { left: 0; }
  .head { font-size: 20px; font-weight: 600; padding: 16px; }
  .row {
    text-align: left; padding: 12px 16px; background: transparent; border: 0;
    border-top: 1px solid rgba(0,0,0,0.04);
    font: inherit; cursor: pointer;
  }
  .row.primary { font-weight: 500; }
  .row:hover { background: rgba(0,0,0,0.04); }
  .section-label { padding: 12px 16px 4px; font-size: 12px; opacity: 0.5; text-transform: uppercase; }
  .empty { padding: 8px 16px; opacity: 0.5; }
  .spacer { flex: 1; }
  .overlay {
    position: fixed; inset: 0;
    background: rgba(0,0,0,0.3); border: 0;
    z-index: 55;
  }
  @media (prefers-color-scheme: dark) {
    .drawer { background: var(--drawer-bg, #1c1c1e); }
    .row:hover { background: rgba(255,255,255,0.06); }
    .row { border-top-color: rgba(255,255,255,0.06); }
  }
</style>
