<script lang="ts">
  import type { WorkspaceItem } from '../lib/repository'
  import { t, type MessageKey } from '../lib/strings'

  let {
    item,
    disabled = false,
    canPlace = false,
    canReopen = false,
    onPlace,
    onOpen,
    onReopen,
    onRelink,
  }: {
    item: WorkspaceItem
    disabled?: boolean
    canPlace?: boolean
    canReopen?: boolean
    onPlace(item: WorkspaceItem): void
    onOpen(item: WorkspaceItem): void
    onReopen(item: WorkspaceItem): void
    onRelink(item: WorkspaceItem): void
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

<article class="card" class:orphan={item.orphan}>
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
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--card);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--shadow) 8%, transparent);
  }
  .card.orphan { border-style: dashed; }
  .body { flex: 1; min-width: 0; }
  .title-line { display: flex; align-items: center; gap: 7px; min-width: 0; }
  h3 { margin: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; font-weight: 650; }
  p { margin: 6px 0 0; color: var(--muted); font-size: 12.5px; line-height: 1.45; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state, .badge { flex: none; border-radius: 999px; padding: 2px 7px; font-size: 10.5px; font-weight: 600; }
  .state { background: var(--chip); color: var(--muted-strong); }
  .badge.proof { background: var(--proof-bg); color: var(--proof-fg); }
  .badge.warning { background: var(--warn-bg); color: var(--warn-fg); }
  .actions { display: flex; gap: 6px; flex: none; }
  button { font: inherit; cursor: pointer; }
  button:disabled { cursor: default; opacity: 0.45; }
  .quiet, .place { border-radius: 8px; padding: 6px 10px; font-size: 12px; font-weight: 600; }
  .quiet { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .quiet:hover:not(:disabled) { background: var(--hover); }
  .place { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  .place:hover:not(:disabled) { filter: brightness(1.06); }
</style>
