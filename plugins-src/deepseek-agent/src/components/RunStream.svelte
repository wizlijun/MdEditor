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
  .stream { flex: 1; overflow: auto; padding: 10px 12px; font-size: 13px; line-height: 1.55; }
  .tool {
    font-family: ui-monospace, SFMono-Regular, monospace;
    font-size: 11px;
    opacity: 0.75;
    padding: 2px 0;
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
    font-size: 11px;
    padding: 2px 0;
    opacity: 0.9;
  }
  .p-rejected, .p-cancelled { color: #b58900; }
  .p-allowed { opacity: 0.7; }
  .brief { opacity: 0.75; }
  .text { white-space: pre-wrap; padding: 4px 0; }
</style>
