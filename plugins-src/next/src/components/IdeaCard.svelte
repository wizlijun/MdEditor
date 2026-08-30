<script lang="ts">
  import type { WorkspaceItem } from '../lib/repository'
  import { t, type MessageKey } from '../lib/strings'

  let {
    item,
    disabled = false,
    canPlace = false,
    canReopen = false,
    canDrag = false,
    dragging = false,
    onPlace,
    onOpen,
    onReopen,
    onRelink,
    onDragStart,
    onDragEnd,
  }: {
    item: WorkspaceItem
    disabled?: boolean
    canPlace?: boolean
    canReopen?: boolean
    canDrag?: boolean
    dragging?: boolean
    onPlace(item: WorkspaceItem): void
    onOpen(item: WorkspaceItem): void
    onReopen(item: WorkspaceItem): void
    onRelink(item: WorkspaceItem): void
    onDragStart?(item: WorkspaceItem): void
    onDragEnd?(): void
  } = $props()

  const statusKey = $derived(`status.${item.state}` as MessageKey)
  const detail = $derived.by(() => {
    const projection = item.projection
    if (!projection) return ''
    switch (projection.state) {
      case 'wip': return projection.next_action
      case 'waiting': return `${projection.waiting_for} · ${projection.review_at}`
      case 'dormant': return projection.wake_trigger
      case 'closed': return projection.target ?? projection.reason ?? projection.result ?? ''
      case 'unsupported': return projection.unsupported_actions.join(', ')
      case 'capture': return ''
    }
  })
</script>

<article
  class="card"
  class:orphan={item.orphan}
  class:dragging
  data-item-key={item.key}
  draggable={canDrag && !disabled}
  ondragstart={(event) => {
    if (!canDrag || disabled) {
      event.preventDefault()
      return
    }
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('text/plain', item.key)
    }
    onDragStart?.(item)
  }}
  ondragend={() => onDragEnd?.()}
>
  <div class="body">
    <div class="title-line">
      <h3>{item.title}</h3>
      <span class="state">{t(statusKey)}</span>
      {#if item.proofed}<span class="badge proof">{t('badge.proofed')}</span>{/if}
      {#if item.orphan}<span class="badge warning">{t('badge.orphan')}</span>{/if}
      {#if item.state === 'unsupported'}<span class="badge warning">{t('badge.unsupported')}</span>{/if}
    </div>
    {#if detail}<p>{detail}</p>{/if}
  </div>
  <div class="actions">
    {#if item.path && !item.orphan}
      <button class="quiet" disabled={disabled} onclick={() => onOpen(item)}>{t('common.open')}</button>
    {/if}
    {#if item.orphan}
      <button class="quiet" disabled={disabled} onclick={() => onRelink(item)}>{t('action.relink')}</button>
    {/if}
    {#if canReopen}
      <button class="quiet" disabled={disabled} onclick={() => onReopen(item)}>{t('action.reopen')}</button>
    {/if}
    {#if canPlace}
      <button class="place" disabled={disabled} onclick={() => onPlace(item)}>{t('action.place')}</button>
    {/if}
  </div>
</article>

<style>
  .card {
    display: grid;
    align-content: space-between;
    gap: 14px;
    width: 100%;
    min-height: 128px;
    box-sizing: border-box;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--card);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--shadow) 8%, transparent);
  }
  .card.orphan { border-style: dashed; }
  .card[draggable="true"] { cursor: grab; }
  .card[draggable="true"]:active { cursor: grabbing; }
  .card.dragging { opacity: 0.42; }
  .body { min-width: 0; }
  .title-line { display: flex; align-items: flex-start; flex-wrap: wrap; gap: 7px; min-width: 0; }
  h3 { width: 100%; margin: 0; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; font-size: 14px; line-height: 1.38; font-weight: 650; }
  p { margin: 8px 0 0; color: var(--muted); font-size: 12.5px; line-height: 1.45; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  .state, .badge { flex: none; border-radius: 999px; padding: 2px 7px; font-size: 10.5px; font-weight: 600; }
  .state { background: var(--chip); color: var(--muted-strong); }
  .badge.proof { background: var(--proof-bg); color: var(--proof-fg); }
  .badge.warning { background: var(--warn-bg); color: var(--warn-fg); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  button { font: inherit; cursor: pointer; }
  button:disabled { cursor: default; opacity: 0.45; }
  .quiet, .place { border-radius: 8px; padding: 6px 10px; font-size: 12px; font-weight: 600; }
  .quiet { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .quiet:hover:not(:disabled) { background: var(--hover); }
  .place { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  .place:hover:not(:disabled) { filter: brightness(1.06); }
</style>
