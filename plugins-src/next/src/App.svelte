<script lang="ts">
  import { onMount } from 'svelte'
  import CreateIdeaSheet from './components/CreateIdeaSheet.svelte'
  import IdeaCard from './components/IdeaCard.svelte'
  import PlaceSheet from './components/PlaceSheet.svelte'
  import RelinkSheet from './components/RelinkSheet.svelte'
  import { bridge, toast } from './lib/bridge'
  import type { PlaceInput } from './lib/events'
  import { itemSearchText, type WorkspaceItem } from './lib/repository'
  import type { IdeaSource } from './lib/source'
  import {
    createIdea,
    open as openItem,
    place,
    refresh,
    relink,
    reopen,
    state as store,
  } from './lib/store.svelte'
  import { setLocale, t, type MessageKey } from './lib/strings'
  import { isDormantDue } from './lib/view'

  type Lane = 'capture' | 'wip' | 'waiting' | 'dormant' | 'closed'
  type Route = PlaceInput['route']

  interface LaneView {
    id: Lane
    title: MessageKey
    empty: MessageKey
    items: WorkspaceItem[]
    count: string
  }

  setLocale(bridge().locale)

  let showPlaced = $state(false)
  let creating = $state(false)
  let search = $state('')
  let placing = $state<WorkspaceItem | null>(null)
  let placementRoute = $state<Route | undefined>()
  let relinking = $state<WorkspaceItem | null>(null)
  let dragging = $state<WorkspaceItem | null>(null)
  let dragOver = $state<Lane | null>(null)
  let dragPress: { item: WorkspaceItem; startX: number; startY: number; pointerId: number } | null = null
  let ghostX = $state(0)
  let ghostY = $state(0)
  let previewing = $state<WorkspaceItem | null>(null)
  let previewAnchor: HTMLElement | null = null
  let previewTip = $state<HTMLElement | null>(null)
  let previewX = $state(12)
  let previewY = $state(12)
  const laneElements = $state<Partial<Record<Lane, HTMLElement>>>({})
  const dragThreshold = 5

  const workspace = $derived(store.workspace)
  const blocked = $derived(Boolean(workspace?.readOnlyError || workspace?.projection.hasBlockingIssues))
  const interactionDisabled = $derived(store.saving || blocked)
  const newIdeaShortcut = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
    ? '⌘N'
    : 'Ctrl+N'
  const repair = $derived(workspace?.items.filter((item) => item.state === 'unsupported' || (item.state === 'capture' && item.orphan)) ?? [])

  function filter(items: WorkspaceItem[]): WorkspaceItem[] {
    const query = search.trim().toLocaleLowerCase()
    return query ? items.filter((item) => itemSearchText(item).includes(query)) : items
  }

  const lanes = $derived.by<LaneView[]>(() => {
    if (!workspace) return []
    const searching = Boolean(search.trim())
    const capture = workspace.capture.filter((item) => !item.orphan)
    const dormant = searching || showPlaced
      ? workspace.dormant
      : workspace.dormant.filter((item) => isDormantDue(item))
    const closed = searching || showPlaced ? workspace.closed : []
    const visibleCapture = filter(searching ? capture : capture.slice(0, 10))
    const visibleDormant = filter(dormant)
    const visibleClosed = filter(closed)
    return [
      { id: 'capture', title: 'section.capture', empty: 'empty.capture', items: visibleCapture, count: String(visibleCapture.length) },
      { id: 'wip', title: 'section.wip', empty: 'empty.wip', items: filter(workspace.wip), count: t('count.wip', { count: workspace.wip.length }) },
      { id: 'waiting', title: 'section.waiting', empty: 'empty.waiting', items: filter(workspace.waiting), count: t('count.waiting', { count: workspace.waiting.length }) },
      { id: 'dormant', title: 'status.dormant', empty: 'empty.dormant', items: visibleDormant, count: String(visibleDormant.length) },
      { id: 'closed', title: 'status.closed', empty: 'empty.closed', items: visibleClosed, count: String(visibleClosed.length) },
    ]
  })

  async function report(action: () => Promise<void>, messageKey: 'error.save' | 'error.open' | 'error.load' | 'error.create') {
    try {
      await action()
    } catch (error) {
      await toast('error', t(messageKey), String(error))
    }
  }

  function openCreation() {
    if (store.loading || store.saving || placing || relinking) return
    creating = true
  }

  async function submitIdea(body: string) {
    await report(async () => {
      await createIdea(body)
      creating = false
    }, 'error.create')
  }

  function closePlacement() {
    placing = null
    placementRoute = undefined
  }

  async function submitPlacement(input: PlaceInput) {
    if (!placing) return
    const item = placing
    await report(async () => {
      if (item.state === 'dormant' || item.state === 'closed') await reopen(item)
      await place(item, input)
      closePlacement()
    }, 'error.save')
  }

  async function submitRelink(source: IdeaSource) {
    if (!relinking) return
    const item = relinking
    await report(async () => {
      await relink(item, source)
      relinking = null
    }, 'error.save')
  }

  function doOpen(item: WorkspaceItem) {
    void report(() => openItem(item), 'error.open')
  }

  function doReopen(item: WorkspaceItem) {
    void report(() => reopen(item), 'error.save')
  }

  function canDrop(item: WorkspaceItem, lane: Lane): boolean {
    if (interactionDisabled || item.state === 'unsupported' || item.state === lane) return false
    if (lane === 'capture') return item.state === 'dormant' || item.state === 'closed'
    return true
  }

  function routeFor(lane: Exclude<Lane, 'capture'>): Route {
    if (lane === 'wip') return 'commit'
    if (lane === 'waiting') return 'wait'
    if (lane === 'dormant') return 'park'
    return 'settle'
  }

  function dragStart(item: WorkspaceItem, event: PointerEvent) {
    if (dragPress || interactionDisabled || item.state === 'unsupported') return
    closePreview()
    dragPress = {
      item,
      startX: event.clientX,
      startY: event.clientY,
      pointerId: event.pointerId,
    }
    ghostX = event.clientX
    ghostY = event.clientY
  }

  function dragEnd() {
    dragPress = null
    dragging = null
    dragOver = null
  }

  function closePreview() {
    previewing = null
    previewAnchor = null
    previewTip = null
  }

  function previewStart(item: WorkspaceItem, anchor: HTMLElement) {
    if (!item.body?.trim() || dragging) return
    previewing = item
    previewAnchor = anchor
    const rect = anchor.getBoundingClientRect()
    const margin = 12
    const gap = 10
    const width = Math.min(380, Math.max(0, window.innerWidth - margin * 2))
    const maxHeight = Math.min(480, Math.max(0, window.innerHeight - margin * 2))
    const beside = rect.right + gap
    const alternate = rect.left - width - gap
    previewX = Math.max(margin, Math.min(
      beside + width <= window.innerWidth - margin ? beside : alternate,
      window.innerWidth - width - margin,
    ))
    previewY = Math.max(margin, Math.min(rect.top, window.innerHeight - maxHeight - margin))
  }

  function previewEnd(event: PointerEvent | FocusEvent) {
    const next = event.relatedTarget
    if (next instanceof Node && (previewAnchor?.contains(next) || previewTip?.contains(next))) return
    closePreview()
  }

  function laneAtPoint(x: number, y: number): Lane | null {
    for (const lane of ['capture', 'wip', 'waiting', 'dormant', 'closed'] as const) {
      const rect = laneElements[lane]?.getBoundingClientRect()
      if (rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) return lane
    }
    return null
  }

  function moveToLane(item: WorkspaceItem, lane: Lane) {
    if (!canDrop(item, lane)) return
    if (lane === 'capture') {
      doReopen(item)
      return
    }
    placementRoute = routeFor(lane)
    placing = item
  }

  function pointerMove(event: PointerEvent) {
    if (!dragPress || event.pointerId !== dragPress.pointerId) return
    if (!dragging) {
      const distance = Math.hypot(event.clientX - dragPress.startX, event.clientY - dragPress.startY)
      if (distance < dragThreshold) return
      if (interactionDisabled) {
        dragEnd()
        return
      }
      dragging = dragPress.item
    }
    event.preventDefault()
    ghostX = event.clientX
    ghostY = event.clientY
    const lane = laneAtPoint(event.clientX, event.clientY)
    dragOver = lane && canDrop(dragging, lane) ? lane : null
  }

  function pointerUp(event: PointerEvent) {
    if (!dragPress || event.pointerId !== dragPress.pointerId) return
    const item = dragging
    const hovered = item ? laneAtPoint(event.clientX, event.clientY) : null
    const lane = item && hovered && canDrop(item, hovered) ? hovered : null
    dragEnd()
    if (item && lane) moveToLane(item, lane)
  }

  onMount(() => {
    void report(refresh, 'error.load')
    const onFocus = () => {
      if (!store.saving) void report(refresh, 'error.load')
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && (dragPress || dragging)) {
        event.preventDefault()
        dragEnd()
        return
      }
      if (event.key === 'Escape' && previewing) {
        event.preventDefault()
        closePreview()
        return
      }
      if (event.key.toLocaleLowerCase() !== 'n' || (!event.metaKey && !event.ctrlKey) || event.altKey || event.shiftKey) return
      event.preventDefault()
      if (!creating) openCreation()
    }
    window.addEventListener('focus', onFocus)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('focus', onFocus)
      window.removeEventListener('keydown', onKey)
    }
  })
</script>

<svelte:window
  onpointermove={pointerMove}
  onpointerup={pointerUp}
  onpointercancel={dragEnd}
  onblur={() => { dragEnd(); closePreview() }}
  onresize={closePreview}
/>

<main class="app">
  <header class="topbar">
    <div>
      <h1>{t('app.title')}</h1>
      <p>{t('app.value')}</p>
    </div>
    <div class="top-actions">
      <button type="button" data-action="new-idea" class="new-idea" aria-keyshortcuts="Meta+N Control+N" disabled={store.loading || store.saving} onclick={openCreation}>
        <span aria-hidden="true">＋</span>
        <span>{t('action.newIdea')}</span>
        <kbd>{newIdeaShortcut}</kbd>
      </button>
      <button type="button" class="refresh" disabled={store.loading || store.saving} onclick={() => void report(refresh, 'error.load')}>
        ↻ <span>{t('common.refresh')}</span>
      </button>
    </div>
  </header>

  {#if store.loading && !workspace}
    <div class="loading">{t('common.loading')}</div>
  {:else if workspace}
    <div class="content">
      {#if blocked}
        <div class="banner danger">
          <strong>{t('warning.readOnly')}</strong>
          <span>{workspace.readOnlyError ?? workspace.projection.issues.map((issue) => issue.message).join(' · ')}</span>
        </div>
      {/if}
      {#if workspace.scanErrors.length}
        <div class="banner warning" title={workspace.scanErrors.join('\n')}>{t('common.error')} · {workspace.scanErrors[0]}</div>
      {/if}
      {#if workspace.projection.wipAtLimit}<div class="banner calm">{t('warning.wip')}</div>{/if}
      {#if workspace.projection.waitingExceeded}<div class="banner calm">{t('warning.waiting')}</div>{/if}

      {#if repair.length}
        <section class="repair">
          <div class="section-head"><h2>{t('section.repair')}</h2></div>
          <div class="repair-cards">
            {#each repair as item (item.key)}
              <IdeaCard
                {item}
                disabled={interactionDisabled}
                canPlace={item.state === 'capture'}
                onPlace={(value) => { placing = value; placementRoute = undefined }}
                onOpen={doOpen}
                onReopen={doReopen}
                onRelink={(value) => relinking = value}
              />
            {/each}
          </div>
        </section>
      {/if}

      <div class="board-tools">
        <input class="search" type="search" bind:value={search} placeholder={t('search.placeholder')} />
        <button class:active={showPlaced} onclick={() => showPlaced = !showPlaced}>
          {showPlaced ? t('action.hidePlaced') : t('action.findPlaced')}
        </button>
      </div>

        <div class="board-scroll" onscroll={closePreview}>
        <div class="board">
          {#each lanes as lane (lane.id)}
            <section
              bind:this={laneElements[lane.id]}
              class="lane"
              aria-label={t(lane.title)}
              class:over={dragOver === lane.id}
              class:available={Boolean(dragging && canDrop(dragging, lane.id))}
              data-lane={lane.id}
            >
              <header class="lane-head">
                <h2>{t(lane.title)}</h2>
                <span>{lane.count}</span>
              </header>
              <div class="lane-body" role="list" aria-label={t(lane.title)}>
                {#each lane.items as item (item.key)}
                  <div role="listitem">
                    <IdeaCard
                      {item}
                      disabled={interactionDisabled}
                      canDrag={item.state !== 'unsupported'}
                      dragging={dragging?.key === item.key}
                      canPlace={item.state === 'capture' || item.state === 'wip' || item.state === 'waiting'}
                      canReopen={item.state === 'dormant' || item.state === 'closed'}
                      onPlace={(value) => { placing = value; placementRoute = undefined }}
                      onOpen={doOpen}
                      onReopen={doReopen}
                      onRelink={(value) => relinking = value}
                      onDragStart={dragStart}
                      onPreviewStart={previewStart}
                      onPreviewEnd={previewEnd}
                    />
                  </div>
                {:else}
                  <p class="empty">{t(lane.empty)}</p>
                {/each}
              </div>
            </section>
          {/each}
        </div>
      </div>
      <p class="drag-help">{t('board.dragHelp')}</p>
    </div>
  {/if}
</main>

{#if dragging}
  <div class="drag-ghost" style="left:{ghostX}px; top:{ghostY}px">{dragging.title}</div>
{/if}

{#if previewing?.body}
  <aside
    bind:this={previewTip}
    id="idea-preview-tip"
    class="idea-preview-tip"
    role="tooltip"
    style="left:{previewX}px; top:{previewY}px"
    onpointerleave={previewEnd}
  ><pre>{previewing.body}</pre></aside>
{/if}

{#if placing}
  <PlaceSheet item={placing} saving={store.saving} initialRoute={placementRoute} onCancel={closePlacement} onSubmit={submitPlacement} />
{/if}

{#if relinking}
  <RelinkSheet item={relinking} saving={store.saving} onCancel={() => relinking = null} onSubmit={submitRelink} />
{/if}

{#if creating}
  <CreateIdeaSheet
    ideaDir={workspace?.ideaDir ?? 'inbox/ideas'}
    saving={store.saving}
    onCancel={() => creating = false}
    onSubmit={submitIdea}
  />
{/if}

<style>
  :global(:root) {
    color-scheme: light dark;
    --bg: #f7f7f8;
    --card: #fff;
    --sheet: #fff;
    --input: #fff;
    --fg: #1d1d1f;
    --muted: #77777d;
    --muted-strong: #5d5d62;
    --line: #e3e3e7;
    --line-strong: #c9c9cf;
    --chip: #efeff2;
    --hover: #f2f2f4;
    --accent: #1677ff;
    --accent-soft: #eaf3ff;
    --proof-bg: #e8f5ed;
    --proof-fg: #247447;
    --warn-bg: #fff2d8;
    --warn-fg: #8b5b00;
    --danger: #c72c36;
    --shadow: #000;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #171719;
      --card: #222225;
      --sheet: #252528;
      --input: #1d1d20;
      --fg: #f2f2f4;
      --muted: #99999f;
      --muted-strong: #b7b7bc;
      --line: #343438;
      --line-strong: #4b4b51;
      --chip: #303034;
      --hover: #2c2c30;
      --accent: #4093ff;
      --accent-soft: #17385e;
      --proof-bg: #173a28;
      --proof-fg: #7bdca3;
      --warn-bg: #443316;
      --warn-fg: #f3c96f;
      --danger: #ff7b85;
      --shadow: #000;
    }
  }
  :global(html), :global(body) { height: 100%; }
  :global(body) { margin: 0; background: var(--bg); color: var(--fg); font: 13px/1.45 -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'PingFang SC', 'Segoe UI', sans-serif; }
  :global(button), :global(input), :global(textarea), :global(select) { font-family: inherit; }
  .app { min-height: 100vh; }
  .topbar { position: sticky; top: 0; z-index: 5; display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 20px 28px 16px; border-bottom: 1px solid var(--line); background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(18px); }
  h1 { margin: 0; font-size: 24px; line-height: 1.1; letter-spacing: -0.02em; }
  .topbar p { margin: 5px 0 0; color: var(--muted); }
  .top-actions { flex: none; display: flex; align-items: center; gap: 8px; }
  .new-idea, .refresh { min-height: 34px; box-sizing: border-box; border-radius: 9px; padding: 7px 10px; font-weight: 650; cursor: pointer; }
  .new-idea { display: flex; align-items: center; gap: 7px; border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  .new-idea:hover:not(:disabled) { filter: brightness(1.06); }
  .new-idea kbd { border-radius: 5px; background: color-mix(in srgb, #000 16%, transparent); padding: 1px 5px; font: 10px/1.5 inherit; }
  .refresh { flex: none; border: 1px solid var(--line); border-radius: 9px; background: var(--card); color: var(--fg); padding: 7px 10px; font-weight: 600; cursor: pointer; }
  .refresh:hover:not(:disabled) { background: var(--hover); }
  .refresh:disabled, .new-idea:disabled { opacity: 0.45; cursor: default; }
  .loading { min-height: 60vh; display: grid; place-items: center; color: var(--muted); }
  .content { padding: 22px 28px 48px; }
  .banner, .repair, .board-tools, .board-scroll, .drag-help { max-width: 1500px; margin-right: auto; margin-left: auto; }
  .banner { display: grid; gap: 3px; margin-bottom: 10px; padding: 10px 12px; border-radius: 10px; font-size: 12px; box-sizing: border-box; }
  .banner span { opacity: 0.75; overflow-wrap: anywhere; }
  .banner.danger { background: color-mix(in srgb, var(--danger) 12%, var(--card)); color: var(--danger); }
  .banner.warning { background: var(--warn-bg); color: var(--warn-fg); }
  .banner.calm { background: var(--chip); color: var(--muted-strong); }
  .repair { margin-bottom: 16px; }
  .section-head { margin: 0 2px 8px; }
  .section-head h2 { margin: 0; font-size: 13px; }
  .repair-cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(248px, 1fr)); gap: 8px; }
  .board-tools { display: flex; gap: 8px; margin-bottom: 12px; }
  .search { flex: 1; min-width: 160px; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 10px; background: var(--input); color: var(--fg); padding: 9px 11px; outline: none; }
  .search:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .board-tools button { flex: none; border: 1px solid var(--line); border-radius: 10px; background: var(--card); color: var(--fg); padding: 8px 12px; font-weight: 650; cursor: pointer; }
  .board-tools button:hover { background: var(--hover); }
  .board-tools button.active { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .board-scroll { overflow-x: auto; padding: 2px 2px 14px; }
  .board { display: grid; grid-template-columns: repeat(5, minmax(248px, 1fr)); gap: 12px; min-width: 1312px; }
  .lane { min-width: 0; min-height: 420px; margin: 0; padding: 10px; border: 1px solid var(--line); border-radius: 16px; background: color-mix(in srgb, var(--chip) 48%, transparent); transition: border-color 120ms ease, background 120ms ease; }
  .lane.available { border-style: dashed; }
  .lane.over { border-color: var(--accent); background: var(--accent-soft); box-shadow: inset 0 0 0 1px var(--accent); }
  .drag-ghost { position: fixed; z-index: 60; max-width: 240px; transform: translate(10px, 10px); overflow: hidden; border: 1px solid var(--accent); border-radius: 10px; background: var(--card); color: var(--fg); box-shadow: 0 8px 24px color-mix(in srgb, var(--shadow) 22%, transparent); padding: 9px 12px; font-size: 12px; font-weight: 650; opacity: 0.88; pointer-events: none; text-overflow: ellipsis; white-space: nowrap; }
  .idea-preview-tip { position: fixed; z-index: 55; width: min(380px, calc(100vw - 24px)); max-height: min(480px, calc(100vh - 24px)); box-sizing: border-box; overflow: auto; overscroll-behavior: contain; border: 1px solid var(--line-strong); border-radius: 12px; background: color-mix(in srgb, var(--card) 96%, transparent); color: var(--fg); box-shadow: 0 14px 38px color-mix(in srgb, var(--shadow) 28%, transparent); padding: 14px 16px; backdrop-filter: blur(18px); }
  .idea-preview-tip pre { margin: 0; font: 12.5px/1.55 inherit; overflow-wrap: anywhere; white-space: pre-wrap; }
  .lane-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 2px 4px 10px; }
  .lane-head h2 { margin: 0; font-size: 13px; letter-spacing: 0.01em; }
  .lane-head span { min-width: 20px; border-radius: 999px; background: var(--card); color: var(--muted); padding: 2px 7px; text-align: center; font-size: 11px; }
  .lane-body { display: grid; align-content: start; gap: 8px; min-height: 360px; }
  .empty { margin: 0; padding: 14px 5px; color: var(--muted); font-size: 12px; }
  .drag-help { margin-top: 8px; color: var(--muted); font-size: 11.5px; }
  @media (max-width: 660px) {
    .topbar { padding: 16px 18px 13px; }
    .topbar p { max-width: 440px; }
    .refresh span { display: none; }
    .content { padding: 18px 16px 36px; }
    .board { grid-template-columns: repeat(5, 248px); }
  }
</style>
