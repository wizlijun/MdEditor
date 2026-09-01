<script lang="ts">
  import type { WorkspaceItem } from '../lib/repository'
  import { DEFAULT_PRIORITY, localDateAfter } from '../lib/metadata'
  import { projectTagsOf } from '../lib/model'
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
    onSuggestProject,
    onDragStart,
    onPreviewStart,
    onPreviewEnd,
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
    onSuggestProject?(item: WorkspaceItem, project: string): void
    onDragStart?(item: WorkspaceItem, event: PointerEvent): void
    onPreviewStart?(item: WorkspaceItem, anchor: HTMLElement, trigger: 'pointer' | 'focus', tipId: string): void
    onPreviewEnd?(trigger: 'pointer' | 'focus'): void
  } = $props()

  const statusKey = $derived(`status.${item.state}` as MessageKey)
  const hasPreview = $derived(Boolean(item.body?.trim()))
  const domToken = $derived(encodeURIComponent(item.key))
  const titleId = $derived(`idea-card-title-${domToken}`)
  const tipId = $derived(`idea-preview-${domToken}`)
  const projects = $derived(projectTagsOf(item.projection))
  const priority = $derived(item.priority ?? item.task?.priority ?? DEFAULT_PRIORITY)
  const due = $derived(item.due ?? item.task?.due)
  const contexts = $derived(item.contexts ?? item.task?.contexts ?? [])
  const dueState = $derived.by(() => {
    if (!due) return 'none'
    if (item.state !== 'closed' && due < localDateAfter(0)) return 'overdue'
    return 'dated'
  })
  const detail = $derived.by(() => {
    const projection = item.projection
    const taskDetail = item.kind === 'task'
      ? item.description ?? ''
      : ''
    if (!projection) return taskDetail
    switch (projection.state) {
      case 'wip': return projection.next_action
      case 'waiting': return `${projection.waiting_for} · ${projection.review_at}`
      case 'dormant': return projection.wake_trigger
      case 'closed': return projection.target ?? projection.reason ?? projection.result ?? ''
      case 'unsupported': return projection.unsupported_actions.join(', ')
      case 'capture': return taskDetail
    }
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<article
  class="card"
  class:orphan={item.orphan}
  class:dragging
  class:draggable={canDrag && !disabled}
  data-item-key={item.key}
  data-draggable={canDrag && !disabled}
  role="group"
  tabindex={hasPreview ? 0 : undefined}
  aria-labelledby={titleId}
  aria-describedby={hasPreview ? tipId : undefined}
  onpointerenter={(event) => {
    if (hasPreview) onPreviewStart?.(item, event.currentTarget, 'pointer', tipId)
  }}
  onpointerleave={() => onPreviewEnd?.('pointer')}
  onfocusin={(event) => {
    if (hasPreview) onPreviewStart?.(item, event.currentTarget, 'focus', tipId)
  }}
  onfocusout={(event) => {
    const next = event.relatedTarget
    if (next instanceof Node && event.currentTarget.contains(next)) return
    onPreviewEnd?.('focus')
  }}
  onpointerdown={(event) => {
    if (!canDrag || disabled || event.button !== 0) return
    if (event.target instanceof Element && event.target.closest('button')) return
    onDragStart?.(item, event)
  }}
>
  <div class="body">
    <div class="title-line">
      <h3 id={titleId}>{item.title}</h3>
      <span class="state">{t(statusKey)}</span>
      {#each projects.slice(0, 2) as project}
        <span class="badge project" title={projects.join(', ')}>{project}</span>
      {/each}
      {#if projects.length > 2}<span class="badge project" title={projects.join(', ')}>+{projects.length - 2}</span>{/if}
      {#if item.suggestedProject && onSuggestProject}
        <button
          type="button"
          class="badge project-suggestion"
          data-project-suggestion={item.suggestedProject.project}
          title={t('project.suggestion.detail', { terms: item.suggestedProject.matchedTerms.join(' · ') })}
          disabled={disabled}
          onclick={() => onSuggestProject?.(item, item.suggestedProject!.project)}
        >{t('project.suggestion', { project: item.suggestedProject.project })}</button>
      {/if}
      {#if item.kind === 'task'}<span class="badge task">{t('badge.task')}</span>{/if}
      {#if item.generatedBy}<span class="badge agent" title={item.generatedBy}>{t('badge.agent')}</span>{/if}
      {#if item.proofed}<span class="badge proof">{t('badge.proofed')}</span>{/if}
      {#if item.orphan}<span class="badge warning">{t('badge.orphan')}</span>{/if}
      {#if item.state === 'unsupported'}<span class="badge warning">{t('badge.unsupported')}</span>{/if}
    </div>
    <div class="planning" aria-label={t('metadata.label')}>
      <span class="badge priority" data-priority={priority}>{t(`priority.${priority}` as never)}</span>
      {#if dueState === 'none'}
        <span class="badge due muted" data-due="none">{t('badge.dueNone')}</span>
      {:else}
        <span class:overdue={dueState === 'overdue'} class="badge due" data-due={due}>
          {t(dueState === 'overdue' ? 'badge.overdue' : 'badge.due', { date: due! })}
        </span>
      {/if}
      {#if contexts.length}
        {#each contexts.slice(0, 2) as context}<span class="badge context" data-context={context}>{context}</span>{/each}
        {#if contexts.length > 2}<span class="badge context">+{contexts.length - 2}</span>{/if}
      {:else}
        <span class="badge context missing" data-context="missing">{t('badge.contextMissing')}</span>
      {/if}
    </div>
    {#if detail}<p>{detail}</p>{/if}
  </div>
  <div class="actions">
    {#if item.path && !item.orphan}
      <button class="quiet" disabled={disabled} onclick={() => onOpen(item)}>{t('common.open')}</button>
    {/if}
    {#if item.orphan && item.kind !== 'task'}
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
  .card.draggable { cursor: grab; user-select: none; }
  .card.draggable:active { cursor: grabbing; }
  .card.dragging { opacity: 0.42; }
  .card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .body { min-width: 0; }
  .title-line { display: flex; align-items: flex-start; flex-wrap: wrap; gap: 7px; min-width: 0; }
  .planning { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  h3 { width: 100%; margin: 0; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; font-size: 14px; line-height: 1.38; font-weight: 650; }
  p { margin: 8px 0 0; color: var(--muted); font-size: 12.5px; line-height: 1.45; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  .state, .badge { flex: none; border-radius: 999px; padding: 2px 7px; font-size: 10.5px; font-weight: 600; }
  .state { background: var(--chip); color: var(--muted-strong); }
  .badge.proof { background: var(--proof-bg); color: var(--proof-fg); }
  .badge.task { background: var(--accent-soft); color: var(--accent); }
  .badge.agent { border: 1px solid var(--line); background: transparent; color: var(--muted-strong); }
  .badge.project { background: var(--accent-soft); color: var(--accent); }
  .badge.priority { border: 1px solid var(--line-strong); background: var(--chip); color: var(--fg); }
  .badge.due { background: var(--accent-soft); color: var(--accent); }
  .badge.due.muted { background: var(--chip); color: var(--muted-strong); }
  .badge.due.overdue { background: color-mix(in srgb, var(--danger) 13%, var(--card)); color: var(--danger); }
  .badge.context { background: var(--proof-bg); color: var(--proof-fg); }
  .badge.context.missing { background: var(--warn-bg); color: var(--warn-fg); }
  .project-suggestion { border: 1px dashed var(--accent); background: transparent; color: var(--accent); cursor: pointer; }
  .project-suggestion:hover:not(:disabled) { background: var(--accent-soft); }
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
