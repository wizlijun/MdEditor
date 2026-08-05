<!-- src/logs-app.svelte — standalone Logs window (View ▸ View Logs, or tray
     git-sync entry which presets the category filter to git-sync). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { loadLocale, watchLocaleChanges, t } from './lib/i18n/store.svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { createLogsStore } from './lib/logs/logs-store.svelte'
  import type { LogLine } from './lib/logs/console-bridge'

  const store = createLogsStore()
  let sourceFilter = $state<'all' | 'backend' | 'frontend'>('all')
  let levelFilter = $state<'all' | 'debug' | 'info' | 'warn' | 'error'>('all')
  let search = $state('')
  let autoScroll = $state(true)
  let ready = $state(false)
  let logEnd: HTMLDivElement | undefined

  onMount(() => {
    let stop: (() => void) | undefined
    let unlistenLocale: (() => void) | undefined
    ;(async () => {
      await loadLocale()
      try { await getCurrentWindow().setTitle(t('logs.title')) } catch { /* no-op */ }
      // Follow live language switches from the main window's Settings.
      unlistenLocale = await watchLocaleChanges()
      stop = await store.start()
      ready = true
    })()
    return () => { stop?.(); unlistenLocale?.() }
  })

  const PLUGIN_PREFIX = 'plugin:'

  // 'all' matches everything; 'plugin' matches every plugin:<id> (grouped);
  // any other value is an exact category match, including a specific plugin:<id>.
  function matchCategory(line: LogLine): boolean {
    const f = store.categoryFilter
    if (f === 'all') return true
    if (f === 'plugin') return line.category.startsWith(PLUGIN_PREFIX)
    return line.category === f
  }

  // Distinct plugin categories present in the stream, so each plugin id can be
  // selected on its own in the filter (sorted for stable ordering).
  const pluginCategories = $derived(
    [...new Set(store.lines.map((l) => l.category).filter((c) => c.startsWith(PLUGIN_PREFIX)))].sort()
  )

  const filtered = $derived(
    store.lines.filter((l) =>
      (sourceFilter === 'all' || l.source === sourceFilter) &&
      matchCategory(l) &&
      (levelFilter === 'all' || l.level === levelFilter) &&
      (search === '' || l.message.toLowerCase().includes(search.toLowerCase())))
  )

  $effect(() => {
    // Re-run on every filtered change; scroll to bottom when enabled.
    filtered.length
    if (autoScroll) logEnd?.scrollIntoView({ block: 'end' })
  })

  function catClass(cat: string): string {
    if (cat === 'git-sync') return 'cat-git'
    if (cat === 'notification') return 'cat-notif'
    if (cat.startsWith('plugin:')) return 'cat-plugin'
    if (cat === 'frontend') return 'cat-frontend'
    return 'cat-core'
  }
</script>

<div class="logs-root">
  <header class="bar">
    <select bind:value={sourceFilter} title={t('logs.source')} aria-label={t('logs.source')}>
      <option value="all">{t('logs.sources.all')}</option>
      <option value="backend">{t('logs.sources.backend')}</option>
      <option value="frontend">{t('logs.sources.frontend')}</option>
    </select>
    <select bind:value={store.categoryFilter} title={t('logs.category')} aria-label={t('logs.category')}>
      <option value="all">{t('logs.categories.all')}</option>
      <option value="core">{t('logs.categories.core')}</option>
      <option value="git-sync">{t('logs.categories.gitSync')}</option>
      <option value="notification">{t('logs.categories.notification')}</option>
      <option value="frontend">{t('logs.categories.frontend')}</option>
      {#if pluginCategories.length > 0}
        <optgroup label={t('logs.categories.plugin')}>
          <option value="plugin">{t('logs.categories.pluginAll')}</option>
          {#each pluginCategories as cat (cat)}
            <option value={cat}>{cat.slice(PLUGIN_PREFIX.length)}</option>
          {/each}
        </optgroup>
      {/if}
    </select>
    <select bind:value={levelFilter} title={t('logs.level')} aria-label={t('logs.level')}>
      <option value="all">{t('logs.levels.all')}</option>
      <option value="debug">{t('logs.levels.debug')}</option>
      <option value="info">{t('logs.levels.info')}</option>
      <option value="warn">{t('logs.levels.warn')}</option>
      <option value="error">{t('logs.levels.error')}</option>
    </select>
    <input class="search" type="text" placeholder={t('logs.search')} bind:value={search} />
    <label class="auto"><input type="checkbox" bind:checked={autoScroll} />{t('logs.autoScroll')}</label>
    <button onclick={() => store.clear()}>{t('logs.clear')}</button>
  </header>

  <div class="stream">
    {#if ready && filtered.length === 0}
      <div class="empty">{t('logs.empty')}</div>
    {/if}
    {#each filtered as line, i (i)}
      <div class="row">
        <span class="ts">{line.ts}</span>
        <span class="cat {catClass(line.category)}">[{line.category}]</span>
        <span class="src">[{line.source}]</span>
        <span class="lvl lvl-{line.level}">{line.level}</span>
        <span class="msg">{line.message}</span>
      </div>
    {/each}
    <div bind:this={logEnd}></div>
  </div>
</div>

<style>
  /* Theme parity with the main window: Canvas/CanvasText system colors so the
     window follows light/dark like every other surface; accent colors use
     light-dark() pairs (the old values were dark-only VSCode hex codes). The
     monospace stack is intentional — this is a log stream viewer. */
  .logs-root { display: flex; flex-direction: column; height: 100vh; background: Canvas; color: CanvasText; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; }
  .bar { display: flex; gap: 8px; align-items: center; padding: 6px 8px; background: color-mix(in srgb, CanvasText 6%, Canvas); border-bottom: 1px solid color-mix(in srgb, CanvasText 16%, transparent); flex-wrap: wrap; }
  .bar select, .bar input, .bar button { background: color-mix(in srgb, CanvasText 9%, Canvas); color: CanvasText; border: 1px solid color-mix(in srgb, CanvasText 22%, transparent); border-radius: 4px; padding: 2px 6px; font: inherit; }
  .search { flex: 1; min-width: 120px; }
  .auto { display: flex; align-items: center; gap: 4px; white-space: nowrap; }
  .stream { flex: 1; overflow-y: auto; padding: 6px 8px; }
  .empty { opacity: 0.5; padding: 16px; text-align: center; }
  .row { display: flex; gap: 8px; padding: 1px 0; align-items: baseline; }
  .ts { color: color-mix(in srgb, CanvasText 52%, transparent); white-space: nowrap; }
  .src { color: light-dark(#3f7d2d, #6a9955); white-space: nowrap; }
  .cat { white-space: nowrap; }
  .cat-git { color: light-dark(#0b7bd0, #4fc1ff); }
  .cat-notif { color: light-dark(#b8860b, #d7ba7d); }
  .cat-plugin { color: light-dark(#8e44ad, #c586c0); }
  .cat-frontend { color: light-dark(#0e8574, #4ec9b0); }
  .cat-core { color: light-dark(#3f7d2d, #6a9955); }
  .lvl { white-space: nowrap; text-transform: uppercase; }
  .lvl-error { color: light-dark(#c62f2f, #f14c4c); }
  .lvl-warn { color: light-dark(#9a6700, #cca700); }
  .lvl-debug { color: light-dark(#6e6e6e, #888); }
  .lvl-info { color: CanvasText; }
  .msg { white-space: pre-wrap; word-break: break-all; }
</style>
