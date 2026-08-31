<!-- src/plugin-market-app.svelte — standalone Plugin Market window (opened from
     View ▸ Plugin Market or the Settings ▸ Plugins entry button). Bootstraps its
     own webview state, then drives the market commands.

     Restores a small installed-plugin snapshot first, then reconciles it with
     plugin_market_installed. The registry arrives independently and adds the
     available plugins, descriptions, and update state into the same category
     sections. Installed plugins can be toggled, uninstalled, or updated.

     ── MANUAL E2E (do not run the GUI in CI) ──────────────────────────────────
     1. For a local registry, set plugins_v2.registry_url in settings.json to
        your test server.
     2. View ▸ Plugin Market → cached/current installed plugins appear first;
        after the registry responds, every category shows installed and
        available plugins together.
     3. Click Install on an available plugin → the consent modal runs
        plugin_market_preview (verifies the real package) and lists its
        capabilities (vault.write / secrets highlighted). Click Trust & Install.
     4. Install succeeds → both lists re-fetch; reconcile activates it with no
        restart. The main window receives `plugins-changed` and re-fetches
        manifests (enable/disable of existing menu items reflects immediately;
        a brand-new native menu item may still need a restart — known gap).
     5. Toggle enabled on an installed plugin → plugin_market_set_enabled;
        Uninstall → confirm → plugin_market_uninstall; both re-fetch. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { getVersion } from '@tauri-apps/api/app'
  import { confirm } from '@tauri-apps/plugin-dialog'
  import { loadSettings } from './lib/settings.svelte'
  import { i18n, loadLocale, watchLocaleChanges, t } from './lib/i18n/store.svelte'
  import { pushToast } from './lib/toast.svelte'
  import {
    capabilityLabel,
    isSensitiveCapability,
    type RegistryIndex,
    type RegistryEntry,
    type InstalledV2,
    type InstalledRow,
  } from './lib/market/types'
  import { pickAvailable, pickUpdateTo } from './lib/market/select'
  import { readInstalledCache, writeInstalledCache } from './lib/market/cache'
  import { localizedPluginDescription, localizedPluginName } from './lib/market/plugin-text'
  import {
    groupPluginsByCategory,
    pluginCategoryLabelKey,
    type PluginCategory,
  } from './lib/plugins/categories'
  import ConsentModal from './components/market/ConsentModal.svelte'

  type InstalledMarketItem = {
    kind: 'installed'
    id: string
    category?: string | null
    row: InstalledRow
    listing?: RegistryEntry
  }

  type AvailableMarketItem = {
    kind: 'available'
    id: string
    category?: string | null
    entry: RegistryEntry
  }

  type MarketItem = InstalledMarketItem | AvailableMarketItem

  let ready = $state(false)
  let loading = $state(false)
  // Inline notice for flag-off / network errors (not a toast — persistent).
  let notice = $state<string | null>(null)

  let installedRows = $state<InstalledRow[]>([])
  let installedState = $state<InstalledV2[]>([])
  let available = $state<RegistryEntry[]>([])
  let catalogEntries = $state<RegistryEntry[]>([])
  let catalogReady = $state(false)
  let busy = $state<Record<string, boolean>>({})
  // Running app version for min_host selection; null = unknown (fail open).
  let hostVersion: string | null = null
  let refreshSequence = 0
  let catalogById = $derived(new Map(catalogEntries.map((entry) => [entry.id, entry])))
  let marketItems: MarketItem[] = $derived([
    ...installedRows.map((row): InstalledMarketItem => {
      const listing = catalogById.get(row.id)
      return {
        kind: 'installed',
        id: row.id,
        category: row.category ?? listing?.category,
        row,
        listing,
      }
    }),
    ...available.map((entry): AvailableMarketItem => ({
      kind: 'available',
      id: entry.id,
      category: entry.category,
      entry,
    })),
  ])
  let marketGroups = $derived(groupPluginsByCategory(marketItems))

  // Consent modal target (null = closed).
  let consent = $state<{
    id: string
    version: string
    name: string
    description?: string | null
    i18n?: RegistryEntry['i18n']
  } | null>(null)

  onMount(() => {
    let unlisten: (() => void) | null = null
    let unlistenLocale: (() => void) | null = null
    void (async () => {
      try {
        await loadSettings()
        await loadLocale()
        try { await getCurrentWindow().setTitle(t('pluginMarket.windowTitle')) } catch { /* no-op */ }
      } catch (e) {
        console.error('[plugin-market] init failed:', e)
      }
      try { hostVersion = await getVersion() } catch { hostVersion = null }
      const cached = readInstalledCache()
      if (cached.length > 0) applyInstalled(cached, [], false)
      ready = true
      await refresh()
      // The main window emits `plugins-changed` after every mutating op; when it
      // fires from elsewhere (e.g. a CLI install) keep our lists fresh too.
      unlisten = await listen('plugins-changed', () => { void refresh() })
      // Follow live language switches from the main window's Settings.
      unlistenLocale = await watchLocaleChanges()
    })()
    return () => { unlisten?.(); unlistenLocale?.() }
  })

  function setBusy(id: string, v: boolean) {
    busy = { ...busy, [id]: v }
  }

  /** Re-fetch device state first, then enrich it with the remote catalog. */
  async function refresh() {
    const sequence = ++refreshSequence
    loading = true
    notice = null
    // Start both requests together, but apply the device-local result first.
    // The registry can be slow or offline and must never gate installed plugins.
    const installedRequest = invoke<InstalledV2[]>('plugin_market_installed')
    const indexRequest = invoke<RegistryIndex>('plugin_market_index')
    try {
      const localInstalled = await installedRequest
      if (sequence === refreshSequence) applyInstalled(localInstalled, catalogEntries, true)
    } catch (e) {
      if (sequence === refreshSequence) {
        // Keep the cache visible if querying the device fails.
        notice = t('pluginMarket.localStateError', { error: String(e) })
      }
    }

    try {
      const indexJson = await indexRequest
      if (sequence === refreshSequence) applyCatalog(indexJson)
    } catch (e) {
      if (sequence === refreshSequence) {
        // Keep installedRows intact when the network/registry is unavailable.
        notice = friendlyError(String(e))
      }
    } finally {
      if (sequence === refreshSequence) loading = false
    }
  }

  function applyInstalled(
    v2: InstalledV2[],
    entries: RegistryEntry[],
    persist: boolean,
  ) {
    const previousById = new Map(installedState.map((plugin) => [plugin.id, plugin]))
    const listings = new Map(
      pickAvailable(entries, new Set<string>(), hostVersion).map((entry) => [entry.id, entry]),
    )
    installedState = v2.map((plugin) => {
      const listing = listings.get(plugin.id)
      const previous = previousById.get(plugin.id)
      return {
        ...plugin,
        name: plugin.name ?? listing?.name ?? previous?.name ?? plugin.id,
        description: plugin.description ?? listing?.description ?? previous?.description,
        i18n: mergePluginI18n(
          mergePluginI18n(previous?.i18n, listing?.i18n),
          plugin.i18n,
        ),
        category: plugin.category ?? listing?.category ?? previous?.category,
      }
    })

    // The index carries one entry per published VERSION (several per id);
    // pickUpdateTo/pickAvailable collapse that to one row per plugin, choosing
    // the newest version this host satisfies.
    const rows: InstalledRow[] = []
    for (const p of installedState) {
      rows.push({
        kind: 'v2',
        id: p.id,
        name: p.name ?? p.id,
        description: p.description,
        i18n: p.i18n,
        category: p.category,
        version: p.version,
        enabled: p.enabled,
        capabilities: p.capabilities ?? [],
        updateTo: pickUpdateTo(entries, p.id, p.version, hostVersion),
      })
    }
    installedRows = rows.sort((a, b) => a.id.localeCompare(b.id))
    const installedIds = new Set(installedState.map((row) => row.id))
    available = available.filter((entry) => !installedIds.has(entry.id))
    if (persist) writeInstalledCache(installedState)
  }

  function applyCatalog(index: RegistryIndex) {
    const entries = index.plugins ?? []
    catalogEntries = pickAvailable(entries, new Set<string>(), hostVersion)
    // Registry metadata fills category/name gaps in older installed manifests,
    // and the enriched result becomes the next launch's local-first snapshot.
    applyInstalled(installedState, entries, true)
    const installedIds = new Set(installedState.map((row) => row.id))
    available = pickAvailable(entries, installedIds, hostVersion)
    catalogReady = true
  }

  function friendlyError(msg: string): string {
    return t('pluginMarket.networkError', { error: msg })
  }

  // ── Installed actions ──────────────────────────────────────────────────────

  async function toggleEnabled(row: InstalledRow, value: boolean) {
    setBusy(row.id, true)
    try {
      await invoke('plugin_market_set_enabled', { id: row.id, enabled: value })
      await refresh()
    } catch (e) {
      pushToast({ level: 'error', message: friendlyError(String(e)) })
    } finally {
      setBusy(row.id, false)
    }
  }

  async function uninstall(row: InstalledRow, displayName: string) {
    const ok = await confirm(t('pluginMarket.uninstallConfirm', { name: displayName }), {
      title: t('pluginMarket.windowTitle'),
      kind: 'warning',
    })
    if (!ok) return
    setBusy(row.id, true)
    try {
      await invoke('plugin_market_uninstall', { id: row.id, keepData: false })
      pushToast({ level: 'success', message: t('pluginMarket.uninstalled', { name: displayName }) })
      await refresh()
    } catch (e) {
      pushToast({ level: 'error', message: friendlyError(String(e)) })
    } finally {
      setBusy(row.id, false)
    }
  }

  // Update = install the newer version over the current (install commits the
  // new version + reconciles). Runs through the consent modal, same as a fresh
  // install, so the user re-consents to the new version's capabilities.
  function update(item: InstalledMarketItem) {
    if (!item.row.updateTo) return
    consent = {
      id: item.id,
      version: item.row.updateTo,
      name: item.listing?.name ?? item.row.name,
      description: item.listing?.description ?? item.row.description,
      i18n: mergePluginI18n(item.listing?.i18n, item.row.i18n),
    }
  }

  // ── Available actions ──────────────────────────────────────────────────────

  function beginInstall(entry: RegistryEntry) {
    consent = {
      id: entry.id,
      version: entry.version,
      name: entry.name,
      description: entry.description,
      i18n: entry.i18n,
    }
  }

  function onInstalled() {
    const name = consent ? localizedPluginName(consent, i18n.locale) : ''
    consent = null
    pushToast({ level: 'success', message: t('pluginMarket.installed', { name }) })
    void refresh()
  }

  function categoryMark(category: PluginCategory): string {
    const marks: Record<PluginCategory, string> = {
      agents: '✦',
      capture: '+',
      reading: '◫',
      thinking: '◇',
      'import-export': '⇄',
      editing: '⌘',
      other: '•••',
    }
    return marks[category]
  }

  function monogram(name: string): string {
    return Array.from(name.trim())[0]?.toLocaleUpperCase() ?? 'P'
  }

  function mergePluginI18n(
    fallback: RegistryEntry['i18n'],
    primary: InstalledV2['i18n'],
  ): InstalledV2['i18n'] {
    if (!fallback) return primary
    if (!primary) return fallback
    const merged = { ...fallback }
    for (const [locale, catalog] of Object.entries(primary)) {
      merged[locale] = { ...fallback[locale], ...catalog }
    }
    return merged
  }

  function installedTextSource(item: InstalledMarketItem) {
    return {
      id: item.id,
      name: item.row.name,
      description: item.row.description,
      i18n: mergePluginI18n(item.listing?.i18n, item.row.i18n),
    }
  }

  function itemName(item: MarketItem): string {
    return localizedPluginName(
      item.kind === 'installed' ? installedTextSource(item) : item.entry,
      i18n.locale,
    )
  }

  function itemDescription(item: MarketItem): string | null {
    return localizedPluginDescription(
      item.kind === 'installed' ? installedTextSource(item) : item.entry,
      i18n.locale,
    )
  }
</script>

<main aria-busy={loading}>
  {#if !ready}
    <div class="boot"><span class="spinner"></span></div>
  {:else}
    <div class="page-shell">
      <header class="hero">
        <div class="hero-copy">
          <h1>{t('pluginMarket.windowTitle')}</h1>
          <p>{t('pluginMarket.subtitle')}</p>
        </div>
        <button class="refresh" onclick={() => refresh()} disabled={loading}>
          <span class:spinning={loading} aria-hidden="true">↻</span>
          {t('pluginMarket.refresh')}
        </button>
        <div class="summary" aria-live="polite">
          <span class="summary-item"><strong>{installedRows.length}</strong>{t('pluginMarket.installedHeading')}</span>
          {#if catalogReady}
            <span class="summary-divider"></span>
            <span class="summary-item"><strong>{marketItems.length}</strong>{t('pluginMarket.pluginsUnit')}</span>
          {:else if loading}
            <span class="summary-divider"></span>
            <span class="summary-item syncing"><span class="spinner small"></span>{t('pluginMarket.loadingCatalog')}</span>
          {/if}
        </div>
      </header>

      {#if notice}
        <div class="notice" role="status"><span aria-hidden="true">!</span><p>{notice}</p></div>
      {/if}

      <div class="catalog">
        {#if marketGroups.length === 0 && loading}
          <section class="category-block skeleton-block" aria-label={t('pluginMarket.loadingCatalog')}>
            <div class="skeleton category-skeleton"></div>
            <div class="plugin-grid">
              {#each [1, 2, 3] as n (n)}
                <div class="plugin-card skeleton-card">
                  <div class="skeleton skeleton-title"></div>
                  <div class="skeleton skeleton-line"></div>
                  <div class="skeleton skeleton-line short"></div>
                </div>
              {/each}
            </div>
          </section>
        {:else if marketGroups.length === 0}
          <div class="empty-state">
            <span aria-hidden="true">✦</span>
            <h2>{catalogReady ? t('pluginMarket.noneAvailable') : t('pluginMarket.noneInstalled')}</h2>
          </div>
        {:else}
          {#each marketGroups as group (group.key)}
            <section class="category-block" data-category={group.key}>
              <header class="category-header">
                <span class="category-mark" aria-hidden="true">{categoryMark(group.key)}</span>
                <div>
                  <h2>{t(pluginCategoryLabelKey(group.key))}</h2>
                  <p>{group.items.length} {t('pluginMarket.pluginsUnit')}</p>
                </div>
              </header>

              <div class="plugin-grid">
                {#each group.items as item (item.id)}
                  {@const displayName = itemName(item)}
                  {@const displayDescription = itemDescription(item)}
                  <article
                    class="plugin-card"
                    class:installed-card={item.kind === 'installed'}
                    class:update-card={item.kind === 'installed' && !!item.row.updateTo}
                  >
                    {#if item.kind === 'installed'}
                      <div class="card-heading">
                        <span class="plugin-mark" aria-hidden="true">{monogram(displayName)}</span>
                        <div class="plugin-title">
                          <h3>{displayName}</h3>
                          <span class="version">v{item.row.version}</span>
                        </div>
                        {#if item.row.updateTo}
                          <span class="status update-status">
                            {t('pluginMarket.updateAvailable')} · v{item.row.updateTo}
                          </span>
                        {:else}
                          <span class="status" class:enabled={item.row.enabled}>
                            {item.row.enabled ? t('pluginMarket.enabled') : t('pluginMarket.disabled')}
                          </span>
                        {/if}
                      </div>

                      <p class="desc">{displayDescription ?? t('pluginMarket.onDevice')}</p>

                      {#if item.row.capabilities.length > 0}
                        <div class="caps">
                          {#each item.row.capabilities as cap (cap)}
                            <span class="cap" class:sensitive={isSensitiveCapability(cap)}>{capabilityLabel(cap)}</span>
                          {/each}
                        </div>
                      {/if}

                      <footer class="card-footer">
                        <label class="switch-control">
                          <input type="checkbox" checked={item.row.enabled} disabled={busy[item.row.id]}
                            onchange={(e) => toggleEnabled(item.row, (e.currentTarget as HTMLInputElement).checked)} />
                          <span class="switch" aria-hidden="true"><span></span></span>
                          <span class="switch-label">{item.row.enabled ? t('pluginMarket.enabled') : t('pluginMarket.disabled')}</span>
                        </label>
                        <div class="actions">
                          {#if item.row.updateTo}
                            <button class="mini primary update-action" disabled={busy[item.row.id]}
                              onclick={() => update(item)}>
                              {t('pluginMarket.update', { version: item.row.updateTo })}
                            </button>
                          {/if}
                          <button class="mini quiet danger" disabled={busy[item.row.id]} onclick={() => uninstall(item.row, displayName)}>
                            {t('pluginMarket.uninstall')}
                          </button>
                        </div>
                      </footer>
                    {:else}
                      <div class="card-heading">
                        <span class="plugin-mark" aria-hidden="true">{monogram(displayName)}</span>
                        <div class="plugin-title">
                          <h3>{displayName}</h3>
                          <span class="version">v{item.entry.version}</span>
                        </div>
                        <span class="status available-status">{t('pluginMarket.availableHeading')}</span>
                      </div>

                      {#if displayDescription}<p class="desc">{displayDescription}</p>{/if}

                      <footer class="card-footer available-footer">
                        <span class="plugin-id">{item.entry.id}</span>
                        <button class="mini primary" onclick={() => beginInstall(item.entry)}>{t('pluginMarket.install')}</button>
                      </footer>
                    {/if}
                  </article>
                {/each}
              </div>
            </section>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</main>

{#if consent}
  <ConsentModal id={consent.id} version={consent.version} name={consent.name}
                description={consent.description} i18n={consent.i18n}
                onInstalled={onInstalled} onClose={() => (consent = null)} />
{/if}

<style>
  :global(:root) { color-scheme: light dark; }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  main {
    height: 100vh;
    overflow: auto;
    box-sizing: border-box;
    background:
      radial-gradient(circle at 8% -8%, color-mix(in srgb, #6d5dfc 13%, transparent), transparent 34rem),
      radial-gradient(circle at 92% 4%, color-mix(in srgb, #18a7c7 10%, transparent), transparent 28rem),
      Canvas;
  }
  .page-shell { width: min(1180px, calc(100% - 40px)); margin: 0 auto; padding: 24px 0 40px; }
  .boot { min-height: 100%; display: grid; place-items: center; }
  .hero {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 14px 20px;
    align-items: start;
    padding: 6px 2px 20px;
  }
  .hero-copy h1 {
    margin: 0;
    font-size: clamp(28px, 4vw, 36px);
    line-height: 1.02;
    letter-spacing: -0.045em;
    font-weight: 760;
  }
  .hero-copy p {
    max-width: 650px;
    margin: 7px 0 0;
    color: color-mix(in srgb, CanvasText 62%, transparent);
    font-size: 13px;
    line-height: 1.4;
  }
  .summary {
    grid-column: 1 / -1;
    display: inline-flex;
    align-items: center;
    justify-self: start;
    min-height: 30px;
    padding: 0 11px;
    border: 1px solid color-mix(in srgb, CanvasText 9%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, Canvas 72%, transparent);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
    box-shadow: 0 6px 20px color-mix(in srgb, CanvasText 4%, transparent);
  }
  .summary-item {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: color-mix(in srgb, CanvasText 58%, transparent);
    font-size: 11px;
    white-space: nowrap;
  }
  .summary-item strong { color: CanvasText; font-size: 12px; }
  .summary-divider { width: 1px; height: 14px; margin: 0 9px; background: color-mix(in srgb, CanvasText 13%, transparent); }
  .syncing { gap: 7px; }
  .notice {
    display: flex;
    gap: 11px;
    align-items: flex-start;
    margin: 0 0 14px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, #e0a800 24%, transparent);
    border-radius: 14px;
    background: color-mix(in srgb, #e0a800 10%, Canvas);
    color: color-mix(in srgb, CanvasText 82%, transparent);
    font-size: 12.5px;
  }
  .notice > span {
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: #d69b00;
    color: white;
    font-weight: 750;
  }
  .notice p { margin: 2px 0 0; line-height: 1.45; overflow-wrap: anywhere; }
  .catalog { display: grid; gap: 14px; }
  .category-block {
    --accent: #635bff;
    --accent-soft: color-mix(in srgb, var(--accent) 11%, Canvas);
    position: relative;
    overflow: hidden;
    padding: 16px;
    border: 1px solid color-mix(in srgb, var(--accent) 15%, CanvasText 7%);
    border-radius: 18px;
    background:
      radial-gradient(circle at 100% 0, color-mix(in srgb, var(--accent) 10%, transparent), transparent 22rem),
      color-mix(in srgb, Canvas 94%, var(--accent) 6%);
    box-shadow: 0 8px 28px color-mix(in srgb, CanvasText 4%, transparent);
  }
  .category-block[data-category='capture'] { --accent: #ec5d74; }
  .category-block[data-category='reading'] { --accent: #f09a3e; }
  .category-block[data-category='thinking'] { --accent: #8a63e6; }
  .category-block[data-category='import-export'] { --accent: #159c92; }
  .category-block[data-category='editing'] { --accent: #3479db; }
  .category-block[data-category='other'] { --accent: #7a8495; }
  .category-header { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .category-mark {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    flex: 0 0 auto;
    border-radius: 10px;
    background: var(--accent);
    color: white;
    box-shadow: 0 5px 12px color-mix(in srgb, var(--accent) 22%, transparent);
    font-size: 15px;
    font-weight: 650;
    letter-spacing: -0.06em;
  }
  .category-header h2 { margin: 0; font-size: 16px; letter-spacing: -0.02em; }
  .category-header p { margin: 2px 0 0; color: color-mix(in srgb, CanvasText 48%, transparent); font-size: 10px; }
  .plugin-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 9px; }
  .plugin-card {
    min-width: 0;
    min-height: 152px;
    display: flex;
    flex-direction: column;
    padding: 13px;
    border: 1px solid color-mix(in srgb, CanvasText 9%, transparent);
    border-radius: 14px;
    background: color-mix(in srgb, Canvas 92%, transparent);
    box-shadow: 0 2px 3px color-mix(in srgb, CanvasText 3%, transparent);
    transition: transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease;
  }
  .plugin-card:hover {
    transform: translateY(-2px);
    border-color: color-mix(in srgb, var(--accent) 32%, transparent);
    box-shadow: 0 14px 30px color-mix(in srgb, CanvasText 8%, transparent);
  }
  .installed-card { border-color: color-mix(in srgb, var(--accent) 18%, CanvasText 7%); }
  .update-card {
    border-color: #f08a00;
    background:
      linear-gradient(135deg, color-mix(in srgb, #f08a00 13%, Canvas), transparent 58%),
      color-mix(in srgb, Canvas 94%, #f08a00 6%);
    box-shadow: 0 0 0 1px color-mix(in srgb, #f08a00 30%, transparent),
      0 8px 24px color-mix(in srgb, #f08a00 14%, transparent);
  }
  .update-card:hover {
    border-color: #f08a00;
    box-shadow: 0 0 0 1px color-mix(in srgb, #f08a00 42%, transparent),
      0 12px 28px color-mix(in srgb, #f08a00 20%, transparent);
  }
  .card-heading { display: flex; align-items: center; gap: 9px; min-width: 0; }
  .plugin-mark {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: 0 0 auto;
    border-radius: 9px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 13px;
    font-weight: 750;
  }
  .plugin-title { min-width: 0; flex: 1; }
  .plugin-title h3 { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; letter-spacing: -0.01em; }
  .version {
    display: block;
    margin-top: 2px;
    color: color-mix(in srgb, CanvasText 43%, transparent);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 9.5px;
  }
  .status {
    flex: 0 0 auto;
    padding: 3px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, CanvasText 7%, transparent);
    color: color-mix(in srgb, CanvasText 54%, transparent);
    font-size: 9px;
    font-weight: 650;
  }
  .status.enabled,
  .available-status { background: color-mix(in srgb, var(--accent) 12%, transparent); color: color-mix(in srgb, var(--accent) 90%, CanvasText); }
  .update-status {
    background: #e97700;
    color: white;
    box-shadow: 0 3px 10px color-mix(in srgb, #e97700 30%, transparent);
  }
  .desc {
    display: -webkit-box;
    min-height: 34px;
    margin: 9px 0 7px;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    color: color-mix(in srgb, CanvasText 61%, transparent);
    font-size: 11.5px;
    line-height: 1.45;
  }
  .caps { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 7px; }
  .cap {
    max-width: 100%;
    padding: 2px 6px;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in srgb, CanvasText 6%, transparent);
    color: color-mix(in srgb, CanvasText 52%, transparent);
    font-size: 8.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cap.sensitive { background: color-mix(in srgb, #e0a800 15%, transparent); color: color-mix(in srgb, #b06b00 88%, CanvasText); font-weight: 600; }
  .card-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: auto;
    padding-top: 9px;
    border-top: 1px solid color-mix(in srgb, CanvasText 7%, transparent);
  }
  .available-footer { justify-content: flex-end; }
  .plugin-id {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: color-mix(in srgb, CanvasText 38%, transparent);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 9.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .switch-control { position: relative; display: inline-flex; align-items: center; gap: 7px; cursor: pointer; }
  .switch-control input { position: absolute; width: 1px; height: 1px; opacity: 0; }
  .switch { position: relative; width: 30px; height: 18px; border-radius: 999px; background: color-mix(in srgb, CanvasText 18%, transparent); transition: background 140ms ease; }
  .switch > span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.24);
    transition: transform 140ms ease;
  }
  .switch-control input:checked + .switch { background: var(--accent); }
  .switch-control input:checked + .switch > span { transform: translateX(12px); }
  .switch-control input:focus-visible + .switch { outline: 2px solid var(--accent); outline-offset: 2px; }
  .switch-control input:disabled + .switch { opacity: 0.45; }
  .switch-label { color: color-mix(in srgb, CanvasText 58%, transparent); font-size: 10.5px; }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 1px solid color-mix(in srgb, CanvasText 13%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, Canvas 82%, transparent);
    color: CanvasText;
    font: inherit;
    font-size: 12px;
    font-weight: 590;
    cursor: pointer;
    transition: transform 120ms ease, background 120ms ease, opacity 120ms ease;
  }
  button:hover:not(:disabled) { transform: translateY(-1px); background: color-mix(in srgb, CanvasText 7%, Canvas); }
  button:focus-visible { outline: 2px solid var(--accent, #3479db); outline-offset: 2px; }
  button:disabled { opacity: 0.48; cursor: default; }
  .refresh { min-height: 32px; padding: 0 12px; -webkit-backdrop-filter: blur(16px); backdrop-filter: blur(16px); }
  .refresh > span { font-size: 16px; }
  .mini { min-height: 27px; padding: 0 9px; font-size: 10px; }
  .primary { border-color: transparent; background: var(--accent, #3479db); color: white; }
  .primary:hover:not(:disabled) { background: color-mix(in srgb, var(--accent, #3479db) 88%, black); }
  .update-action { background: #e97700; box-shadow: 0 3px 10px color-mix(in srgb, #e97700 22%, transparent); }
  .update-action:hover:not(:disabled) { background: #c95f00; }
  .quiet { border-color: transparent; background: transparent; }
  .danger { color: #d43b54; }
  .danger:hover:not(:disabled) { background: color-mix(in srgb, #d43b54 9%, transparent); }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    min-height: 260px;
    border: 1px dashed color-mix(in srgb, CanvasText 14%, transparent);
    border-radius: 26px;
    color: color-mix(in srgb, CanvasText 45%, transparent);
  }
  .empty-state span { font-size: 30px; }
  .empty-state h2 { margin: 0; font-size: 13px; font-weight: 500; }
  .skeleton-block { --accent: #78808b; }
  .skeleton-card:hover { transform: none; box-shadow: none; }
  .skeleton {
    border-radius: 999px;
    background: linear-gradient(100deg,
      color-mix(in srgb, CanvasText 6%, transparent) 20%,
      color-mix(in srgb, CanvasText 11%, transparent) 40%,
      color-mix(in srgb, CanvasText 6%, transparent) 60%);
    background-size: 220% 100%;
    animation: shimmer 1.5s ease-in-out infinite;
  }
  .category-skeleton { width: 150px; height: 38px; margin-bottom: 24px; }
  .skeleton-title { width: 55%; height: 14px; margin: 8px 0 28px; }
  .skeleton-line { width: 100%; height: 9px; margin-bottom: 10px; }
  .skeleton-line.short { width: 72%; }
  .spinner { width: 18px; height: 18px; border: 2px solid color-mix(in srgb, CanvasText 14%, transparent); border-top-color: color-mix(in srgb, CanvasText 65%, transparent); border-radius: 50%; animation: spin 800ms linear infinite; }
  .spinner.small { width: 11px; height: 11px; border-width: 1.5px; }
  .spinning { display: inline-block; animation: spin 850ms linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes shimmer { to { background-position-x: -220%; } }
  @media (min-width: 920px) {
    .plugin-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  }
  @media (max-width: 680px) {
    .page-shell { width: min(100% - 28px, 1180px); padding: 20px 0 32px; }
    .hero { gap: 12px; padding-bottom: 18px; }
    .hero-copy h1 { font-size: 28px; }
    .refresh { padding: 0 11px; }
    .category-block { padding: 14px; border-radius: 16px; }
    .plugin-grid { grid-template-columns: minmax(0, 1fr); }
  }
  @media (max-width: 520px) {
    .page-shell { width: min(100% - 20px, 1180px); padding-top: 16px; }
    .hero { grid-template-columns: minmax(0, 1fr); }
    .refresh { position: absolute; top: 0; right: 0; }
    .hero-copy { padding-right: 90px; }
    .summary { max-width: 100%; box-sizing: border-box; }
    .category-block { padding: 12px; }
    .category-header { margin-bottom: 10px; }
    .plugin-card { padding: 12px; }
    .card-footer { align-items: flex-end; flex-wrap: wrap; }
    .actions { flex-wrap: wrap; }
  }
  @media (prefers-reduced-motion: reduce) {
    .plugin-card, button, .switch, .switch > span { transition: none; }
    .skeleton, .spinner, .spinning { animation: none; }
  }
</style>
