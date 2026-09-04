<script lang="ts">
  import type { Tab } from '../lib/tabs.svelte'
  import { setMode } from '../lib/tabs.svelte'
  import { t } from '../lib/i18n/store.svelte'

  let { tab }: { tab: Tab } = $props()
</script>

{#if tab.kind !== 'image' && tab.kind !== 'canvas'}
<div class="seg" role="tablist" aria-label={t('mode.editorMode')}>
  <button
    type="button"
    role="tab"
    aria-selected={tab.mode === 'rich'}
    class:active={tab.mode === 'rich'}
    onclick={() => setMode(tab.id, 'rich')}
    title={t('mode.previewRich')}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
      <circle cx="12" cy="12" r="3"/>
    </svg>
  </button>
  <button
    type="button"
    role="tab"
    aria-selected={tab.mode === 'source'}
    class:active={tab.mode === 'source'}
    onclick={() => setMode(tab.id, 'source')}
    title={t('mode.source')}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="16 18 22 12 16 6"/>
      <polyline points="8 6 2 12 8 18"/>
    </svg>
  </button>
</div>
{/if}

<style>
  .seg {
    display: inline-flex;
    align-items: center;
    background: color-mix(in srgb, CanvasText 9%, Canvas);
    border-radius: 8px;
    padding: 2px;
    gap: 0;
  }
  .seg button {
    width: 32px;
    height: 26px;
    border: 0;
    background: transparent;
    color: CanvasText;
    border-radius: 6px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    opacity: 0.5;
    transition: opacity 80ms;
  }
  .seg button:hover { opacity: 0.85; }
  .seg button.active {
    background: Canvas;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    opacity: 1;
  }
  .seg svg { display: block; }
</style>
