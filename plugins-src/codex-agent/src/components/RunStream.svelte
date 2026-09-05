<script lang="ts">
  import type { Item } from '../lib/events'

  let { items }: { items: Item[] } = $props()
  let box: HTMLDivElement | undefined = $state()

  // Stick to the bottom as events arrive — unless the user scrolled up to read
  // something, in which case leave them where they are.
  $effect(() => {
    void items.length
    if (box && box.scrollHeight - box.scrollTop - box.clientHeight < 80) {
      box.scrollTop = box.scrollHeight
    }
  })
</script>

<div class="stream" bind:this={box}>
  {#each items as item, i (i)}
    {#if item.type === 'tool'}
      <div class="tool">
        <span class="tname">{item.name}</span>
        <span class="brief">{item.brief}</span>
      </div>
    {:else if item.type === 'permission'}
      <div class="perm p-{item.decision}">
        <span class="tname">{item.decision}</span>
        <span class="brief">{item.tool}</span>
      </div>
    {:else}
      <div class="text">{item.text}</div>
    {/if}
  {/each}
</div>

<style>
  .stream { flex: 1; min-height: 0; overflow: auto; padding: 18px 20px 24px; font-size: 13px; line-height: 1.6; }
  .tool {
    width: fit-content;
    max-width: 100%;
    margin: 3px 0;
    padding: 4px 8px;
    border: 1px solid var(--window-border, color-mix(in srgb, currentColor 11%, transparent));
    border-radius: 7px;
    background: var(--window-surface, Canvas);
    color: var(--muted-text, currentColor);
    font-family: ui-monospace, SFMono-Regular, monospace;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tname { font-weight: 600; }
  /* A permission decision is the one place the sandbox becomes visible in the
     stream, so it reads louder than a tool line — and a refusal is not an
     error, just a boundary doing its job. */
  .perm {
    font-family: ui-monospace, SFMono-Regular, monospace;
    font-size: 12px;
    padding: 2px 0;
    color: var(--ui-secondary);
  }
  .p-rejected, .p-cancelled { color: var(--ui-warning); }
  .p-allowed { color: var(--ui-secondary); }
  .brief { color: var(--ui-secondary); }
  .text { max-width: 780px; white-space: pre-wrap; overflow-wrap: anywhere; padding: 6px 0; }
</style>
