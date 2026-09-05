<script lang="ts">
  import { openInEditor } from '../lib/bridge'
  import type { MessageKey } from '../lib/strings'

  let { paths, label, oncompact = false }:
    {
      paths: string[]
      label: (k: MessageKey, v?: Record<string, string | number>) => string
      /** History rows are cramped: show a count, not the whole list. */
      oncompact?: boolean
    } = $props()

  let failed = $state('')

  const name = (p: string) => p.split('/').pop() ?? p

  async function open(path: string) {
    failed = ''
    try {
      await openInEditor(path)
    } catch (e) {
      // Most likely the file was moved or deleted after the run.
      failed = e instanceof Error ? e.message : String(e)
    }
  }
</script>

{#if paths.length > 0}
  <div class="artifacts" class:compact={oncompact}>
    <span class="lead">{label('artifacts.label')}</span>
    {#each paths as path (path)}
      <button class="link" onclick={() => open(path)} title={path}>{name(path)}</button>
    {/each}
    {#if failed}<span class="err" role="alert">{failed}</span>{/if}
  </div>
{/if}

<style>
  .artifacts {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .artifacts.compact { padding: 2px 0; border-top: 0; font-size: 12px; }
  .lead { color: var(--ui-secondary); }
  /* A button inherits no font — say so, or these drift out of line. */
  .link {
    font: inherit;
    font-size: inherit;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ui-accent-text);
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
    max-width: min(260px, 100%);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link:hover { text-decoration-thickness: 2px; }
  .err { color: var(--ui-danger); }
</style>
