<script lang="ts">
  import { onMount, tick } from 'svelte'
  import CreateIdeaSheet from './components/CreateIdeaSheet.svelte'
  import CreateTaskSheet, { type TaskDraft } from './components/CreateTaskSheet.svelte'
  import IdeaCard from './components/IdeaCard.svelte'
  import PlaceSheet from './components/PlaceSheet.svelte'
  import RelinkSheet from './components/RelinkSheet.svelte'
  import { bridge, toast } from './lib/bridge'
  import type { PlaceInput } from './lib/events'
  import { projectTagsOf } from './lib/model'
  import { itemSearchText, type WorkspaceItem } from './lib/repository'
  import type { IdeaSource } from './lib/source'
  import {
    createIdea,
    createTask,
    open as openItem,
    place,
    refresh,
    relink,
    reopen,
    state as store,
  } from './lib/store.svelte'
  import { setLocale, t, type MessageKey } from './lib/strings'
  import { isDormantDue, previewPosition } from './lib/view'

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
  let creatingTask = $state(false)
  let search = $state('')
  let selectedProject = $state('')
  let placing = $state<WorkspaceItem | null>(null)
  let placementRoute = $state<Route | undefined>()
  let placementProjects = $state<string[]>([])
  let relinking = $state<WorkspaceItem | null>(null)
  let dragging = $state<WorkspaceItem | null>(null)
  let dragOver = $state<Lane | null>(null)
  let dragPress: {
    item: WorkspaceItem
    anchor: HTMLElement
    tipId: string
    startX: number
    startY: number
    pointerId: number
  } | null = null
  let ghostX = $state(0)
  let ghostY = $state(0)
  let previewing = $state<WorkspaceItem | null>(null)
  let previewAnchor: HTMLElement | null = null
  let previewTip = $state<HTMLElement | null>(null)
  let previewTipId = $state('')
  let previewX = $state(12)
  let previewY = $state(12)
  let previewPositionVersion = 0
  let previewPointerActive = false
  let previewFocusActive = false
  let previewCloseTimer: ReturnType<typeof setTimeout> | null = null
  const laneElements = $state<Partial<Record<Lane, HTMLElement>>>({})
  const dragThreshold = 5

  const workspace = $derived(store.workspace)
  const projectOptions = $derived(workspace?.projectOptions ?? [])
  const activeProject = $derived(projectOptions.includes(selectedProject) ? selectedProject : '')
  const filtering = $derived(Boolean(search.trim() || activeProject))
  const blocked = $derived(Boolean(workspace?.readOnlyError || workspace?.projection.hasBlockingIssues))
  const interactionDisabled = $derived(store.saving || blocked)
  const newIdeaShortcut = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
    ? '⌘N'
    : 'Ctrl+N'
  const newTaskShortcut = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
    ? '⇧⌘N'
    : 'Ctrl+Shift+N'
  const repair = $derived(workspace?.items.filter((item) => (
    item.state === 'unsupported' || (item.state === 'capture' && item.orphan)
  ) && matchesFilters(item)) ?? [])

  $effect(() => {
    if (workspace && selectedProject && !projectOptions.includes(selectedProject)) selectedProject = ''
  })

  function matchesProject(item: WorkspaceItem): boolean {
    if (!activeProject) return true
    const projection = item.projection
    if (projectTagsOf(projection).includes(activeProject)) return true
    return projection?.state === 'closed'
      && projection.exit.kind === 'transferred'
      && projection.exit.via === 'project'
      && projection.target === activeProject
  }

  function matchesFilters(item: WorkspaceItem): boolean {
    if (!matchesProject(item)) return false
    const query = search.trim().toLocaleLowerCase()
    return !query || itemSearchText(item).includes(query)
  }

  function filter(items: WorkspaceItem[]): WorkspaceItem[] {
    return items.filter(matchesFilters)
  }

  function toggleProject(project: string) {
    closePreview()
    selectedProject = activeProject === project ? '' : project
  }

  const lanes = $derived.by<LaneView[]>(() => {
    if (!workspace) return []
    const capture = workspace.capture.filter((item) => !item.orphan)
    const dormant = filtering || showPlaced
      ? workspace.dormant
      : workspace.dormant.filter((item) => isDormantDue(item))
    const closed = filtering || showPlaced ? workspace.closed : []
    const visibleCapture = filter(filtering ? capture : capture.slice(0, 10))
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
    if (store.loading || store.saving || placing || relinking || creatingTask) return
    closePreview()
    creating = true
  }

  function openTaskCreation() {
    if (store.loading || store.saving || placing || relinking || creating) return
    closePreview()
    creatingTask = true
  }

  function openPlacement(item: WorkspaceItem, route?: Route, initialProjects: readonly string[] = []) {
    closePreview()
    placing = item
    placementRoute = route
    placementProjects = [...initialProjects]
  }

  function openRelink(item: WorkspaceItem) {
    closePreview()
    relinking = item
  }

  async function refreshWorkspace() {
    closePreview()
    await refresh()
  }

  async function submitIdea(body: string) {
    await report(async () => {
      await createIdea(body)
      creating = false
    }, 'error.create')
  }

  async function submitTask(input: TaskDraft, markCurrent: boolean) {
    try {
      const result = await createTask(input, markCurrent)
      creatingTask = false
      if (result.refreshError) {
        await toast('warn', t(markCurrent ? 'error.createRefreshCurrent' : 'error.createRefresh'), result.refreshError)
      } else if (result.placementError) {
        await toast('warn', t('error.createCurrent'), result.placementError)
      }
    } catch (error) {
      await toast('error', t('error.createTask'), String(error))
    }
  }

  function closePlacement() {
    placing = null
    placementRoute = undefined
    placementProjects = []
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
    closePreview()
    void report(() => openItem(item), 'error.open')
  }

  function doReopen(item: WorkspaceItem) {
    closePreview()
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
    const anchor = event.currentTarget as HTMLElement
    dragPress = {
      item,
      anchor,
      tipId: anchor.getAttribute('aria-describedby') ?? '',
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
    if (previewCloseTimer !== null) clearTimeout(previewCloseTimer)
    previewCloseTimer = null
    previewPointerActive = false
    previewFocusActive = false
    previewing = null
    previewAnchor = null
    previewTip = null
    previewTipId = ''
    previewPositionVersion += 1
  }

  function cancelPreviewClose() {
    if (previewCloseTimer !== null) clearTimeout(previewCloseTimer)
    previewCloseTimer = null
  }

  async function positionPreview(item: WorkspaceItem, anchor: HTMLElement, version: number) {
    const viewport = { width: window.innerWidth, height: window.innerHeight }
    const fallbackSize = {
      width: Math.min(380, Math.max(0, viewport.width - 24)),
      height: Math.min(480, Math.max(0, viewport.height - 24)),
    }
    const initial = previewPosition(anchor.getBoundingClientRect(), fallbackSize, viewport)
    previewX = initial.x
    previewY = initial.y
    await tick()
    if (version !== previewPositionVersion || previewing !== item || previewAnchor !== anchor || !previewTip) return
    const measured = previewTip.getBoundingClientRect()
    const actualSize = {
      width: measured.width || fallbackSize.width,
      height: measured.height || fallbackSize.height,
    }
    const final = previewPosition(
      anchor.getBoundingClientRect(),
      actualSize,
      { width: window.innerWidth, height: window.innerHeight },
    )
    previewX = final.x
    previewY = final.y
  }

  function previewStart(item: WorkspaceItem, anchor: HTMLElement, trigger: 'pointer' | 'focus', tipId: string) {
    if (!item.body?.trim() || dragPress || dragging) return
    cancelPreviewClose()
    if (previewAnchor !== anchor) {
      previewPointerActive = false
      previewFocusActive = false
    }
    if (trigger === 'pointer') previewPointerActive = true
    else previewFocusActive = true
    previewing = item
    previewAnchor = anchor
    previewTipId = tipId
    previewPositionVersion += 1
    void positionPreview(item, anchor, previewPositionVersion)
  }

  function schedulePreviewClose() {
    if (previewPointerActive || previewFocusActive || previewCloseTimer !== null) return
    previewCloseTimer = setTimeout(() => {
      previewCloseTimer = null
      if (!previewPointerActive && !previewFocusActive) closePreview()
    }, 100)
  }

  function previewEnd(trigger: 'pointer' | 'focus') {
    if (trigger === 'pointer') previewPointerActive = false
    else previewFocusActive = false
    schedulePreviewClose()
  }

  function previewTipEnter() {
    cancelPreviewClose()
    previewPointerActive = true
  }

  function previewTipLeave() {
    previewPointerActive = false
    schedulePreviewClose()
  }

  function previewKeydown(event: KeyboardEvent) {
    if (!previewTip || !previewing || event.target !== previewAnchor) return
    const page = Math.max(120, previewTip.clientHeight * 0.8)
    let next: number | null = null
    if (event.key === 'ArrowDown') next = previewTip.scrollTop + 40
    else if (event.key === 'ArrowUp') next = previewTip.scrollTop - 40
    else if (event.key === 'PageDown') next = previewTip.scrollTop + page
    else if (event.key === 'PageUp') next = previewTip.scrollTop - page
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = previewTip.scrollHeight
    if (next === null) return
    event.preventDefault()
    previewTip.scrollTop = Math.max(0, next)
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
    openPlacement(item, routeFor(lane))
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
      closePreview()
      dragging = dragPress.item
    }
    event.preventDefault()
    ghostX = event.clientX
    ghostY = event.clientY
    const lane = laneAtPoint(event.clientX, event.clientY)
    dragOver = lane && canDrop(dragging, lane) ? lane : null
  }

  function pointerUp(event: PointerEvent) {
    const press = dragPress
    if (!dragPress || event.pointerId !== dragPress.pointerId) return
    const item = dragging
    const hovered = item ? laneAtPoint(event.clientX, event.clientY) : null
    const lane = item && hovered && canDrop(item, hovered) ? hovered : null
    dragEnd()
    if (!item && press && !interactionDisabled && press.tipId) {
      const rect = press.anchor.getBoundingClientRect()
      const pointerInside = event.clientX >= rect.left && event.clientX <= rect.right
        && event.clientY >= rect.top && event.clientY <= rect.bottom
      const focused = press.anchor.contains(document.activeElement)
      if (pointerInside || focused) {
        previewStart(press.item, press.anchor, focused ? 'focus' : 'pointer', press.tipId)
      }
      return
    }
    if (item && lane) moveToLane(item, lane)
  }

  onMount(() => {
    void report(refreshWorkspace, 'error.load')
    const onFocus = () => {
      if (!store.saving) void report(refreshWorkspace, 'error.load')
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
      previewKeydown(event)
      if (event.defaultPrevented) return
      if (event.key.toLocaleLowerCase() !== 'n' || (!event.metaKey && !event.ctrlKey) || event.altKey) return
      event.preventDefault()
      if (event.shiftKey) {
        if (!creatingTask) openTaskCreation()
      } else if (!creating) openCreation()
    }
    window.addEventListener('focus', onFocus)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('focus', onFocus)
      window.removeEventListener('keydown', onKey)
      cancelPreviewClose()
    }
  })
</script>

<svelte:window
  onpointermove={pointerMove}
  onpointerup={pointerUp}
  onpointercancel={dragEnd}
  onblur={() => { dragEnd(); closePreview() }}
  onresize={closePreview}
  onscroll={closePreview}
/>

<main class="app">
  <header class="topbar">
    <div class="title-block">
      <h1>{t('app.title')}</h1>
      <div class="subtitle-row">
        <p>{t('app.value')}</p>
        {#if projectOptions.length}
          <div class="project-filters" role="group" aria-label={t('filter.projects')}>
            {#each projectOptions as project}
              <button
                type="button"
                data-project-filter={project}
                aria-pressed={activeProject === project}
                class:active={activeProject === project}
                onclick={() => toggleProject(project)}
              >{project}</button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    <div class="top-actions">
      <button type="button" data-action="new-task" class="new-task" aria-keyshortcuts="Meta+Shift+N Control+Shift+N" disabled={store.loading || store.saving} onclick={openTaskCreation}>
        <span aria-hidden="true">＋</span>
        <span>{t('action.newTask')}</span>
        <kbd>{newTaskShortcut}</kbd>
      </button>
      <button type="button" data-action="new-idea" class="new-idea" aria-keyshortcuts="Meta+N Control+N" disabled={store.loading || store.saving} onclick={openCreation}>
        <span>{t('action.newIdea')}</span>
        <kbd>{newIdeaShortcut}</kbd>
      </button>
      <button type="button" class="refresh" disabled={store.loading || store.saving} onclick={() => void report(refreshWorkspace, 'error.load')}>
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
                onPlace={(value) => openPlacement(value)}
                onOpen={doOpen}
                onReopen={doReopen}
                onRelink={openRelink}
                onSuggestProject={(value, project) => openPlacement(value, undefined, [project])}
                onPreviewStart={previewStart}
                onPreviewEnd={previewEnd}
              />
            {/each}
          </div>
        </section>
      {/if}

      <div class="board-tools">
        <input class="search" type="search" bind:value={search} placeholder={t('search.placeholder')} />
        {#if !filtering}
          <button class:active={showPlaced} onclick={() => showPlaced = !showPlaced}>
            {showPlaced ? t('action.hidePlaced') : t('action.findPlaced')}
          </button>
        {/if}
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
                      onPlace={(value) => openPlacement(value)}
                      onOpen={doOpen}
                      onReopen={doReopen}
                      onRelink={openRelink}
                      onSuggestProject={(value, project) => openPlacement(value, undefined, [project])}
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
    id={previewTipId}
    class="idea-preview-tip"
    role="tooltip"
    style="left:{previewX}px; top:{previewY}px"
    onpointerenter={previewTipEnter}
    onpointerleave={previewTipLeave}
  ><pre>{previewing.body}</pre></aside>
{/if}

{#if placing}
  <PlaceSheet
    item={placing}
    saving={store.saving}
    initialRoute={placementRoute}
    initialProjects={placementProjects}
    projectOptions={workspace?.projectOptions ?? []}
    onCancel={closePlacement}
    onSubmit={submitPlacement}
  />
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

{#if creatingTask}
  <CreateTaskSheet
    taskDir={workspace?.taskDir ?? 'inbox/tasks'}
    saving={store.saving}
    onCancel={() => creatingTask = false}
    onSubmit={submitTask}
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
  .title-block { min-width: 0; }
  h1 { margin: 0; font-size: 24px; line-height: 1.1; letter-spacing: -0.02em; }
  .subtitle-row { display: flex; align-items: center; min-width: 0; gap: 10px; margin-top: 5px; }
  .topbar p { margin: 0; color: var(--muted); }
  .project-filters { display: flex; align-items: center; flex: 1 1 auto; min-width: 0; max-width: min(680px, 52vw); gap: 5px; overflow-x: auto; scrollbar-width: none; }
  .project-filters::-webkit-scrollbar { display: none; }
  .project-filters button { flex: none; max-width: 180px; overflow: hidden; border: 1px solid var(--line); border-radius: 999px; background: var(--chip); color: var(--muted-strong); padding: 3px 8px; font: inherit; font-size: 11px; font-weight: 600; line-height: 1.35; cursor: pointer; text-overflow: ellipsis; white-space: nowrap; }
  .project-filters button:hover { background: var(--hover); }
  .project-filters button.active { border-color: var(--accent); background: var(--accent-soft); color: var(--accent); }
  .top-actions { flex: none; display: flex; align-items: center; gap: 8px; }
  .new-task, .new-idea, .refresh { min-height: 34px; box-sizing: border-box; border-radius: 9px; padding: 7px 10px; font-weight: 650; cursor: pointer; }
  .new-task, .new-idea { display: flex; align-items: center; gap: 7px; }
  .new-task { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  .new-task:hover:not(:disabled), .new-idea:hover:not(:disabled) { filter: brightness(1.06); }
  .new-idea { border: 1px solid var(--line-strong); background: var(--card); color: var(--fg); }
  .new-task kbd, .new-idea kbd { border-radius: 5px; background: color-mix(in srgb, currentColor 12%, transparent); padding: 1px 5px; font: 10px/1.5 inherit; }
  .refresh { flex: none; border: 1px solid var(--line); border-radius: 9px; background: var(--card); color: var(--fg); padding: 7px 10px; font-weight: 600; cursor: pointer; }
  .refresh:hover:not(:disabled) { background: var(--hover); }
  .refresh:disabled, .new-task:disabled, .new-idea:disabled { opacity: 0.45; cursor: default; }
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
  .idea-preview-tip { position: fixed; z-index: 15; width: min(380px, calc(100vw - 24px)); max-height: min(480px, calc(100vh - 24px)); box-sizing: border-box; overflow: auto; overscroll-behavior: contain; border: 1px solid var(--line-strong); border-radius: 12px; background: color-mix(in srgb, var(--card) 96%, transparent); color: var(--fg); box-shadow: 0 14px 38px color-mix(in srgb, var(--shadow) 28%, transparent); padding: 14px 16px; backdrop-filter: blur(18px); }
  .idea-preview-tip pre { margin: 0; font-family: inherit; font-size: 12.5px; line-height: 1.55; overflow-wrap: anywhere; white-space: pre-wrap; }
  .lane-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 2px 4px 10px; }
  .lane-head h2 { margin: 0; font-size: 13px; letter-spacing: 0.01em; }
  .lane-head span { min-width: 20px; border-radius: 999px; background: var(--card); color: var(--muted); padding: 2px 7px; text-align: center; font-size: 11px; }
  .lane-body { display: grid; align-content: start; gap: 8px; min-height: 360px; }
  .empty { margin: 0; padding: 14px 5px; color: var(--muted); font-size: 12px; }
  .drag-help { margin-top: 8px; color: var(--muted); font-size: 11.5px; }
  @media (max-width: 660px) {
    .topbar { align-items: flex-start; padding: 16px 18px 13px; }
    .topbar p { max-width: 440px; }
    .subtitle-row { display: grid; gap: 6px; }
    .project-filters { width: 100%; max-width: 100%; }
    .refresh span { display: none; }
    .content { padding: 18px 16px 36px; }
    .board { grid-template-columns: repeat(5, 248px); }
  }
</style>
