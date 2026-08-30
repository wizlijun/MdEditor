<script lang="ts">
  import { onMount } from 'svelte'
  import IdeaCard from './components/IdeaCard.svelte'
  import PlaceSheet from './components/PlaceSheet.svelte'
  import RelinkSheet from './components/RelinkSheet.svelte'
  import { bridge, toast } from './lib/bridge'
  import type { PlaceInput } from './lib/events'
  import type { WorkspaceItem } from './lib/repository'
  import type { IdeaSource } from './lib/source'
  import {
    open as openItem,
    place,
    refresh,
    relink,
    reopen,
    state as store,
  } from './lib/store.svelte'
  import { setLocale, t } from './lib/strings'
  import { isDormantDue, placedItems } from './lib/view'

  setLocale(bridge().locale)

  let showCapture = $state(false)
  let showPlaced = $state(false)
  let search = $state('')
  let placing = $state<WorkspaceItem | null>(null)
  let relinking = $state<WorkspaceItem | null>(null)

  const workspace = $derived(store.workspace)
  const blocked = $derived(Boolean(workspace?.readOnlyError || workspace?.projection.hasBlockingIssues))
  const due = $derived(workspace?.dormant.filter((item) => isDormantDue(item)) ?? [])
  const repair = $derived(workspace?.items.filter((item) => item.state === 'capture' && item.orphan) ?? [])
  const found = $derived(workspace ? placedItems(workspace.items, search) : [])

  async function report(action: () => Promise<void>, messageKey: 'error.save' | 'error.open' | 'error.load') {
    try {
      await action()
    } catch (error) {
      await toast('error', t(messageKey), String(error))
    }
  }

  async function submitPlacement(input: PlaceInput) {
    if (!placing) return
    const item = placing
    await report(async () => {
      await place(item, input)
      placing = null
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

  onMount(() => {
    void report(refresh, 'error.load')
    const onFocus = () => {
      if (!store.saving) void report(refresh, 'error.load')
    }
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  })
</script>

<main class="app">
  <header class="topbar">
    <div>
      <h1>{t('app.title')}</h1>
      <p>{t('app.value')}</p>
    </div>
    <button class="refresh" disabled={store.loading || store.saving} onclick={() => void report(refresh, 'error.load')}>
      ↻ <span>{t('common.refresh')}</span>
    </button>
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
      {#if workspace.projection.wipAtLimit}
        <div class="banner calm">{t('warning.wip')}</div>
      {/if}
      {#if workspace.projection.waitingExceeded}
        <div class="banner calm">{t('warning.waiting')}</div>
      {/if}

      {#if repair.length}
        <section>
          <div class="section-head"><h2>{t('section.repair')}</h2></div>
          <div class="cards">
            {#each repair as item (item.key)}
              <IdeaCard {item} disabled={store.saving || blocked} canPlace onPlace={(value) => placing = value} onOpen={doOpen} onReopen={doReopen} onRelink={(value) => relinking = value} />
            {/each}
          </div>
        </section>
      {/if}

      <section>
        <div class="section-head">
          <h2>{t('section.wip')}</h2>
          <span>{t('count.wip', { count: workspace.wip.length })}</span>
        </div>
        <div class="cards">
          {#each workspace.wip as item (item.key)}
            <IdeaCard {item} disabled={store.saving || blocked} canPlace onPlace={(value) => placing = value} onOpen={doOpen} onReopen={doReopen} onRelink={(value) => relinking = value} />
          {:else}
            <p class="empty">{t('empty.wip')}</p>
          {/each}
        </div>
      </section>

      <section>
        <div class="section-head">
          <h2>{t('section.waiting')}</h2>
          <span>{t('count.waiting', { count: workspace.waiting.length })}</span>
        </div>
        <div class="cards">
          {#each workspace.waiting as item (item.key)}
            <IdeaCard {item} disabled={store.saving || blocked} canPlace onPlace={(value) => placing = value} onOpen={doOpen} onReopen={doReopen} onRelink={(value) => relinking = value} />
          {:else}
            <p class="empty">{t('empty.waiting')}</p>
          {/each}
        </div>
      </section>

      {#if due.length}
        <section>
          <div class="section-head"><h2>{t('section.resurfaced')}</h2></div>
          <div class="cards">
            {#each due as item (item.key)}
              <IdeaCard {item} disabled={store.saving || blocked} canReopen onPlace={(value) => placing = value} onOpen={doOpen} onReopen={doReopen} onRelink={(value) => relinking = value} />
            {/each}
          </div>
        </section>
      {/if}

      <div class="disclosure-actions">
        <button class:active={showCapture} onclick={() => showCapture = !showCapture}>
          {showCapture ? t('action.hideCapture') : t('action.placeOne')}
        </button>
        <button class:active={showPlaced} onclick={() => showPlaced = !showPlaced}>
          {showPlaced ? t('action.hidePlaced') : t('action.findPlaced')}
        </button>
      </div>

      {#if showCapture}
        <section class="disclosed">
          <div class="section-head"><h2>{t('section.capture')}</h2></div>
          <div class="cards">
            {#each workspace.capture.slice(0, 10) as item (item.key)}
              <IdeaCard {item} disabled={store.saving || blocked} canPlace onPlace={(value) => placing = value} onOpen={doOpen} onReopen={doReopen} onRelink={(value) => relinking = value} />
            {:else}
              <p class="empty">{t('empty.capture')}</p>
            {/each}
          </div>
        </section>
      {/if}

      {#if showPlaced}
        <section class="disclosed">
          <div class="section-head"><h2>{t('section.placed')}</h2></div>
          <input class="search" type="search" bind:value={search} placeholder={t('search.placeholder')} />
          <div class="cards">
            {#each found as item (item.key)}
              <IdeaCard
                {item}
                disabled={store.saving || blocked}
                canReopen={item.state === 'dormant' || item.state === 'closed'}
                canPlace={item.state === 'capture'}
                onPlace={(value) => placing = value}
                onOpen={doOpen}
                onReopen={doReopen}
                onRelink={(value) => relinking = value}
              />
            {:else}
              <p class="empty">{t('empty.search')}</p>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  {/if}
</main>

{#if placing}
  <PlaceSheet item={placing} saving={store.saving} onCancel={() => placing = null} onSubmit={submitPlacement} />
{/if}

{#if relinking}
  <RelinkSheet item={relinking} saving={store.saving} onCancel={() => relinking = null} onSubmit={submitRelink} />
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
  .refresh { flex: none; border: 1px solid var(--line); border-radius: 9px; background: var(--card); color: var(--fg); padding: 7px 10px; font-weight: 600; cursor: pointer; }
  .refresh:hover:not(:disabled) { background: var(--hover); }
  .refresh:disabled { opacity: 0.45; }
  .loading { min-height: 60vh; display: grid; place-items: center; color: var(--muted); }
  .content { max-width: 860px; margin: 0 auto; padding: 22px 28px 48px; }
  section { margin: 0 0 24px; }
  .section-head { display: flex; align-items: baseline; gap: 8px; margin: 0 2px 9px; }
  .section-head h2 { margin: 0; font-size: 13px; letter-spacing: 0.02em; }
  .section-head span { color: var(--muted); font-size: 12px; }
  .cards { display: grid; gap: 8px; }
  .empty { margin: 0; padding: 12px 2px; color: var(--muted); font-size: 12.5px; }
  .banner { display: grid; gap: 3px; margin-bottom: 10px; padding: 10px 12px; border-radius: 10px; font-size: 12px; }
  .banner span { opacity: 0.75; overflow-wrap: anywhere; }
  .banner.danger { background: color-mix(in srgb, var(--danger) 12%, var(--card)); color: var(--danger); }
  .banner.warning { background: var(--warn-bg); color: var(--warn-fg); }
  .banner.calm { background: var(--chip); color: var(--muted-strong); }
  .disclosure-actions { display: flex; gap: 8px; margin: 4px 0 22px; }
  .disclosure-actions button { border: 1px solid var(--line); border-radius: 10px; background: var(--card); color: var(--fg); padding: 8px 12px; font-weight: 650; cursor: pointer; }
  .disclosure-actions button:hover { background: var(--hover); }
  .disclosure-actions button.active { border-color: var(--accent); color: var(--accent); background: var(--accent-soft); }
  .disclosed { padding-top: 2px; }
  .search { box-sizing: border-box; width: 100%; margin: 0 0 10px; border: 1px solid var(--line-strong); border-radius: 10px; background: var(--input); color: var(--fg); padding: 9px 11px; outline: none; }
  .search:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  @media (max-width: 660px) {
    .topbar { padding: 16px 18px 13px; }
    .topbar p { max-width: 440px; }
    .refresh span { display: none; }
    .content { padding: 18px 16px 36px; }
  }
</style>
