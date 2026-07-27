<script lang="ts">
  import { folderView, toggleExpanded, filterByViewMode, applyHideFolders, displayNameFor, ensureTitle, type FolderEntry } from '../lib/folder-view.svelte'
  import { t } from '../lib/i18n/store.svelte'
  import FolderTreeNode from './FolderTreeNode.svelte'

  let {
    entry,
    depth,
    activePath,
    onOpen,
    onContextMenu,
    renamingPath = null,
    onRenameCommit,
    onRenameCancel,
  }: {
    entry: FolderEntry
    depth: number
    activePath: string | null
    onOpen: (path: string) => void
    onContextMenu: (e: MouseEvent, entry: FolderEntry) => void
    renamingPath?: string | null
    onRenameCommit?: (entry: FolderEntry, name: string) => void
    onRenameCancel?: () => void
  } = $props()

  // Escape unmounts the input, which fires blur with the (stale) value → a
  // double-commit. Guard with a local flag set by Escape before it cancels, so
  // the trailing blur is skipped. Reset each time a fresh rename input mounts.
  let cancelled = false
  $effect(() => {
    if (renamingPath === entry.path) cancelled = false
  })
  function commitFromInput(value: string) {
    if (cancelled) return
    cancelled = true   // 提交后置位:拦截 unmount 触发的尾随 blur 重复提交
    onRenameCommit?.(entry, value)
  }
  function cancelRename() {
    cancelled = true
    onRenameCancel?.()
  }

  // While filtering, folders that survived the filter are force-expanded so
  // matches deep in the tree are revealed without manual clicking.
  let filtering = $derived(!!folderView.filter.trim())
  let expanded = $derived(
    filtering ? folderView.filterVisible.has(entry.path) : folderView.expanded.has(entry.path)
  )
  let children = $derived.by<FolderEntry[]>(() => {
    const all = folderView.entriesCache.get(entry.path) ?? []
    const filtered = filtering ? all.filter((c) => folderView.filterVisible.has(c.path)) : all
    return applyHideFolders(filterByViewMode(filtered, folderView.viewMode), folderView.hideFolders)
  })
  let isActive = $derived(!entry.isDir && entry.path === activePath)

  // 视图模式下的显示名（只改可见文字；重命名/悬浮/置顶仍用真实 entry.name）
  let label = $derived(displayNameFor(entry, folderView.viewMode, folderView.titleCache.get(entry.path)?.title))
  // markdown 模式：对可见 md 行惰性读 H1（按 mtime 缓存，改了才重读）
  $effect(() => {
    if (folderView.viewMode !== 'markdown' || entry.isDir || entry.kind !== 'markdown') return
    const c = folderView.titleCache.get(entry.path)
    if (c && c.mtime === (entry.mtime ?? 0)) return
    void ensureTitle(entry)
  })

  function onRowClick() {
    if (entry.isDir) { toggleExpanded(entry.path); return }
    const target = folderView.viewMode === 'notes' && entry.hasNote && entry.notePath ? entry.notePath : entry.path
    onOpen(target)
  }
</script>

<button
  class="node"
  class:active={isActive}
  style="padding-left: {8 + depth * 14}px"
  onclick={onRowClick}
  oncontextmenu={(e) => onContextMenu(e, entry)}
  title={entry.name}
>
  {#if entry.isDir}
    <svg class="chev" class:open={expanded} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <polyline points="9 18 15 12 9 6" />
    </svg>
    <svg class="icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  {:else}
    <span class="chev spacer"></span>
    {#if entry.isOutlineNote}
      <svg class="icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="8" y1="13" x2="16" y2="13" />
        <line x1="8" y1="17" x2="13" y2="17" />
      </svg>
    {:else}
      <svg class="icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
      </svg>
    {/if}
  {/if}
  {#if renamingPath === entry.path}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="rename-input"
      type="text"
      value={entry.name}
      autofocus
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        e.stopPropagation()
        if (e.key === 'Enter') { e.preventDefault(); commitFromInput((e.currentTarget as HTMLInputElement).value) }
        else if (e.key === 'Escape') { e.preventDefault(); cancelRename() }
      }}
      onblur={(e) => commitFromInput((e.currentTarget as HTMLInputElement).value)}
    />
  {:else}
    <span class="label">{label}</span>
  {/if}
  {#if entry.pinned}
    <span class="pin-badge" title="pinned" aria-hidden="true">
      <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M14 4v5l3 3v2h-5v5l-1 1-1-1v-5H5v-2l3-3V4h-1V2h8v2z" />
      </svg>
    </span>
  {/if}
  {#if entry.hasNote && entry.notePath}
    <span class="note-badge" role="button" tabindex="-1" title={t('folderView.openNote')}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.stopPropagation(); onOpen(entry.notePath!) } }}
      onclick={(e) => { e.stopPropagation(); onOpen(entry.notePath!) }}>
      <!-- brand ✦ sparkle (same path as the app icon / context-menu note icon) -->
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <path transform="translate(12 12) scale(0.083) translate(-185.5 -203)"
          d="M 185.49318,76.468676 C 202.86539,165.0158 220.23759,183.99019 301.30788,202.96457 220.23759,221.93895 202.86539,240.91333 185.49318,329.46046 168.12097,240.91333 150.74877,221.93895 69.67847,202.96457 150.74877,183.99019 168.12097,165.0158 185.49318,76.468676 Z"
          fill="#f59e0b" />
      </svg>
      {#if entry.answeredCount}
        <span class="answer-count" title={t('folderView.newAnswers')}>{entry.answeredCount}</span>
      {/if}
    </span>
  {/if}
</button>

{#if entry.isDir && expanded}
  {#each children as child (child.path)}
    <FolderTreeNode
      entry={child}
      depth={depth + 1}
      {activePath}
      {onOpen}
      {onContextMenu}
      {renamingPath}
      {onRenameCommit}
      {onRenameCancel}
    />
  {/each}
{/if}

<style>
  .node {
    display: flex; align-items: center; gap: 4px;
    width: 100%; box-sizing: border-box;
    text-align: left; padding: 3px 8px; border: 0; background: transparent;
    font: inherit; font-size: 13px; line-height: 1.4; cursor: pointer;
    white-space: nowrap; overflow: hidden;
  }
  .node:hover { background: rgba(0,0,0,0.05); }
  .node.active { background: rgba(0,0,0,0.1); font-weight: 500; }
  .chev { flex: 0 0 auto; width: 14px; height: 14px; opacity: 0.55; transition: transform 0.12s ease; }
  .chev.open { transform: rotate(90deg); }
  .chev.spacer { display: inline-block; visibility: hidden; }
  .icon { flex: 0 0 auto; display: block; opacity: 0.75; }
  .label { overflow: hidden; text-overflow: ellipsis; flex: 1; min-width: 0; }
  .rename-input {
    flex: 1; min-width: 0; font: inherit; font-size: 13px;
    padding: 0 2px; border: 1px solid var(--accent-color, #4a80d4);
    border-radius: 3px; background: Canvas; color: CanvasText; outline: none;
  }
  .pin-badge { flex: 0 0 auto; display: inline-flex; opacity: 0.55; }
  .note-badge { flex: 0 0 auto; display: inline-flex; align-items: center; opacity: 0.9; padding: 1px; border-radius: 3px; }
  .note-badge:hover { opacity: 1; background: rgba(0,0,0,0.08); }
  .answer-count {
    font-size: 9px; font-weight: 700; line-height: 13px;
    color: #fff; background: #2da44e;
    border-radius: 7px; padding: 0 4px; margin-left: 2px;
  }
  @media (prefers-color-scheme: dark) {
    .node:hover { background: rgba(255,255,255,0.07); }
    .node.active { background: rgba(255,255,255,0.13); }
    .note-badge:hover { background: rgba(255,255,255,0.1); }
  }
</style>
