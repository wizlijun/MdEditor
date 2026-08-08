<script lang="ts">
  import { outline } from '../../lib/outline/store.svelte'
  import { backlinksFor } from '../../lib/outline/backlinks'
  import { openFile } from '../../lib/tabs.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import InlineRender from './InlineRender.svelte'
  import { basename } from '../../lib/paths'

  let { page = null, excludeFile = null }: { page?: string | null; excludeFile?: string | null } = $props()
  let hits = $derived.by(() => {
    void outline.version
    if (!page || !outline.backlinkIndex) return []
    return backlinksFor(outline.backlinkIndex, page).filter(h => h.file !== excludeFile)
  })
</script>

<section class="backlinks">
  <h3>{t('outline.backlinks')} <span class="count">{hits.length}</span></h3>
  {#each hits as hit}
    <button class="hit" onclick={() => void openFile(hit.file)}>
      <span class="file">{basename(hit.file)}</span>
      <span class="text"><InlineRender content={hit.text} /></span>
    </button>
  {/each}
  {#if hits.length === 0}<p class="none">{t('outline.noBacklinks')}</p>{/if}
</section>

<style>
  .backlinks { border-top: 1px solid var(--border-color, #3333); padding: 8px; }
  h3 { font-size: 12px; margin: 0 0 6px; opacity: 0.7; }
  .count { opacity: 0.6; font-weight: normal; }
  .hit { display: block; width: 100%; text-align: left; background: none; border: none;
    padding: 4px 6px; border-radius: 4px; cursor: pointer; color: inherit; font-size: 12px; }
  .hit:hover { background: var(--hover-bg, #8881); }
  .file { opacity: 0.6; margin-right: 6px; }
  .none { opacity: 0.5; font-size: 12px; }
</style>
