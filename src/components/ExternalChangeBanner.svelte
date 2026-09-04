<script lang="ts">
  import type { Tab } from '../lib/tabs.svelte'
  import {
    reloadFromDisk, overwriteOnDisk, dismissExternalBanner,
    saveActive, saveAs, closeTab, activate, isManagedMemoryTab,
  } from '../lib/tabs.svelte'
  import { pickSaveCanvasFile, pickSaveFile, confirmDirtyClose } from '../lib/dialogs'
  import { t } from '../lib/i18n/store.svelte'

  let { tab }: { tab: Tab } = $props()
  let memoryReadOnly = $derived(isManagedMemoryTab(tab))

  async function onSaveAs() {
    const path = tab.kind === 'canvas'
      ? await pickSaveCanvasFile(tab.filePath)
      : await pickSaveFile(tab.filePath)
    if (!path) return
    if (tab.kind === 'canvas') {
      const { confirmCanvasSaveAsReferences } = await import('../lib/canvas/save-as')
      if (!await confirmCanvasSaveAsReferences(tab, path)) return
    }
    await saveAs(tab.id, path)
  }

  async function onRecreate() {
    activate(tab.id)
    await saveActive()
  }

  async function onCloseTab() {
    await closeTab(tab.id, confirmDirtyClose)
  }
</script>

{#if !tab.externalBannerDismissed}
  {#if tab.externalState === 'changed'}
    <div class="banner changed" role="status" aria-live="polite">
      <span class="msg">{t('externalChange.modified', { title: tab.title })}</span>
      {#if tab.pendingExternal}
        <button class="action" onclick={() => reloadFromDisk(tab.id)}>{t('externalChange.reload')}</button>
      {/if}
      {#if !memoryReadOnly}
        <button class="action" onclick={() => overwriteOnDisk(tab.id)}>{t('externalChange.overwrite')}</button>
        <button class="action" onclick={onSaveAs}>{t('common.saveAs')}</button>
      {/if}
      <button class="dismiss" aria-label={t('common.dismiss')}
              onclick={() => dismissExternalBanner(tab.id)}>×</button>
    </div>
  {:else if tab.externalState === 'deleted'}
    <div class="banner deleted" role="status" aria-live="polite">
      <span class="msg">{t('externalChange.deleted', { title: tab.title })}</span>
      {#if !memoryReadOnly}
        <button class="action" onclick={onRecreate}>{t('externalChange.recreate')}</button>
        <button class="action" onclick={onSaveAs}>{t('common.saveAs')}</button>
      {/if}
      <button class="action" onclick={onCloseTab}>{t('externalChange.closeTab')}</button>
      <button class="dismiss" aria-label={t('common.dismiss')}
              onclick={() => dismissExternalBanner(tab.id)}>×</button>
    </div>
  {/if}
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    font-size: 12px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 15%, transparent);
  }
  .banner.changed {
    background: #fff3cd;
    color: #664d03;
  }
  .banner.deleted {
    background: #fff3cd;
    color: #842029;
    border-left: 3px solid #d33;
  }
  .msg { flex: 1; }
  .action {
    padding: 3px 10px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, currentColor 35%, transparent);
    background: rgba(255,255,255,0.5);
    color: inherit;
    cursor: pointer;
    font-size: 11px;
  }
  .action:hover { background: rgba(255,255,255,0.85); }
  .dismiss {
    background: transparent;
    border: 0;
    color: inherit;
    cursor: pointer;
    font-size: 16px;
    padding: 0 4px;
    opacity: 0.6;
  }
  .dismiss:hover { opacity: 1; }
</style>
