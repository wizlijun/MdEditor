<!-- src/plugin-market-app.svelte — standalone Plugin Market window (opened from
     View ▸ Plugin Market or the Settings ▸ Plugins entry button). Bootstraps its
     own webview state, then drives the market commands.

     Installed section lists installed plugins (plugin_market_installed, toggled
     via plugin_market_set_enabled; also uninstall + update).

     ── MANUAL E2E (do not run the GUI in CI) ──────────────────────────────────
     1. For a local registry, set plugins_v2.registry_url in settings.json to
        your test server.
     2. View ▸ Plugin Market → the window opens; Installed shows the current
        plugins, Available lists registry entries not yet installed.
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
  import { loadLocale, watchLocaleChanges, t } from './lib/i18n/store.svelte'
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
  import ConsentModal from './components/market/ConsentModal.svelte'

  let ready = $state(false)
  let loading = $state(false)
  // Inline notice for flag-off / network errors (not a toast — persistent).
  let notice = $state<string | null>(null)

  let installedRows = $state<InstalledRow[]>([])
  let available = $state<RegistryEntry[]>([])
  let busy = $state<Record<string, boolean>>({})
  // Running app version for min_host selection; null = unknown (fail open).
  let hostVersion: string | null = null

  // Consent modal target (null = closed).
  let consent = $state<{ id: string; version: string; name: string } | null>(null)

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

  /** Re-fetch both lists. Flag-off / network errors surface as an inline notice. */
  async function refresh() {
    loading = true
    notice = null
    try {
      // v2 installed + registry index. v1 built-ins are never listed here.
      const [v2Installed, indexJson] = await Promise.all([
        invoke<InstalledV2[]>('plugin_market_installed'),
        invoke<RegistryIndex>('plugin_market_index'),
      ])
      buildLists(v2Installed, indexJson)
    } catch (e) {
      // Any market command may Err (flag off, or network). Surface the notice;
      // the window stays usable even with the registry unreachable.
      notice = friendlyError(String(e))
      buildLists([], null)
    } finally {
      loading = false
    }
  }

  function buildLists(
    v2: InstalledV2[],
    index: RegistryIndex | null,
  ) {
    // The index carries one entry per published VERSION (several per id);
    // pickUpdateTo/pickAvailable collapse that to one row per plugin, choosing
    // the newest version this host satisfies.
    const entries = index?.plugins ?? []
    const rows: InstalledRow[] = []
    for (const p of v2) {
      rows.push({
        kind: 'v2',
        id: p.id,
        name: p.name ?? p.id,
        version: p.version,
        enabled: p.enabled,
        capabilities: p.capabilities ?? [],
        updateTo: pickUpdateTo(entries, p.id, p.version, hostVersion),
      })
    }
    installedRows = rows.sort((a, b) => a.name.localeCompare(b.name))

    // Available = not-installed plugins, one row per id.
    const installedIds = new Set(rows.map((r) => r.id))
    available = pickAvailable(entries, installedIds, hostVersion)
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

  async function uninstall(row: InstalledRow) {
    const ok = await confirm(t('pluginMarket.uninstallConfirm', { name: row.name }), {
      title: t('pluginMarket.windowTitle'),
      kind: 'warning',
    })
    if (!ok) return
    setBusy(row.id, true)
    try {
      await invoke('plugin_market_uninstall', { id: row.id, keepData: false })
      pushToast({ level: 'success', message: t('pluginMarket.uninstalled', { name: row.name }) })
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
  function update(row: InstalledRow) {
    if (!row.updateTo) return
    consent = { id: row.id, version: row.updateTo, name: row.name }
  }

  // ── Available actions ──────────────────────────────────────────────────────

  function beginInstall(entry: RegistryEntry) {
    consent = { id: entry.id, version: entry.version, name: entry.name }
  }

  function onInstalled() {
    const name = consent?.name ?? ''
    consent = null
    pushToast({ level: 'success', message: t('pluginMarket.installed', { name }) })
    void refresh()
  }
</script>

<main>
  {#if !ready}
    <p class="msg">…</p>
  {:else}
    <header>
      <h1>{t('pluginMarket.windowTitle')}</h1>
      <button class="refresh" onclick={() => refresh()} disabled={loading}>
        {t('pluginMarket.refresh')}
      </button>
    </header>

    {#if notice}
      <p class="notice">{notice}</p>
    {/if}

    <section>
      <h2>{t('pluginMarket.installedHeading')}</h2>
      {#if installedRows.length === 0}
        <p class="empty">{t('pluginMarket.noneInstalled')}</p>
      {:else}
        {#each installedRows as row (row.id)}
          <div class="card">
            <div class="line">
              <label class="toggle">
                <input type="checkbox" checked={row.enabled}
                       disabled={busy[row.id]}
                       onchange={(e) => toggleEnabled(row, (e.currentTarget as HTMLInputElement).checked)} />
                <span class="name">{row.name}</span>
              </label>
              <span class="version">{row.version}</span>
              <span class="badge">v2</span>
              <span class="spacer"></span>
              {#if row.updateTo}
                <button class="mini primary" disabled={busy[row.id]} onclick={() => update(row)}>
                  {t('pluginMarket.update', { version: row.updateTo })}
                </button>
              {/if}
              <button class="mini danger" disabled={busy[row.id]} onclick={() => uninstall(row)}>
                {t('pluginMarket.uninstall')}
              </button>
            </div>
            {#if row.capabilities.length > 0}
              <div class="caps">
                {#each row.capabilities as cap (cap)}
                  <span class="cap" class:sensitive={isSensitiveCapability(cap)}>{capabilityLabel(cap)}</span>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </section>

    <section>
      <h2>{t('pluginMarket.availableHeading')}</h2>
      {#if available.length === 0}
        <p class="empty">{t('pluginMarket.noneAvailable')}</p>
      {:else}
        {#each available as entry (entry.id)}
          <div class="card">
            <div class="line">
              <span class="name">{entry.name}</span>
              <span class="version">{entry.version}</span>
              <span class="spacer"></span>
              <button class="mini primary" onclick={() => beginInstall(entry)}>
                {t('pluginMarket.install')}
              </button>
            </div>
            {#if entry.description}
              <p class="desc">{entry.description}</p>
            {/if}
          </div>
        {/each}
      {/if}
    </section>
  {/if}
</main>

{#if consent}
  <ConsentModal id={consent.id} version={consent.version} name={consent.name}
                onInstalled={onInstalled} onClose={() => (consent = null)} />
{/if}

<style>
  /* Opt both windows into light/dark so Canvas/CanvasText follow the OS. */
  :global(:root) { color-scheme: light dark; }
  :global(body) { margin: 0; font-family: -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif; background: Canvas; color: CanvasText; }
  main { height: 100vh; overflow: auto; padding: 16px 20px; box-sizing: border-box; }
  header { display: flex; align-items: center; gap: 12px; margin-bottom: 8px; }
  h1 { font-size: 16px; margin: 0; flex: 1; }
  h2 { font-size: 13px; margin: 18px 0 8px; color: color-mix(in srgb, CanvasText 65%, transparent);
    text-transform: uppercase; letter-spacing: 0.5px; }
  .msg { color: color-mix(in srgb, CanvasText 55%, transparent); font-size: 13px; padding: 20px; }
  .notice {
    font-size: 12.5px; padding: 10px 12px; border-radius: 8px; margin: 4px 0 8px;
    background: color-mix(in srgb, #e0a800 14%, transparent);
    color: color-mix(in srgb, CanvasText 88%, transparent);
  }
  .empty { font-size: 12px; color: color-mix(in srgb, CanvasText 55%, transparent); }
  .card {
    padding: 10px 12px; border-radius: 9px; margin-bottom: 8px;
    border: 1px solid color-mix(in srgb, CanvasText 12%, transparent);
  }
  .line { display: flex; align-items: center; gap: 8px; }
  .toggle { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  .name { font-weight: 600; font-size: 13px; }
  .version { font-size: 11px; font-family: ui-monospace, monospace;
    color: color-mix(in srgb, CanvasText 55%, transparent); }
  .badge { font-size: 10px; padding: 1px 6px; border-radius: 999px;
    background: color-mix(in srgb, CanvasText 10%, transparent);
    color: color-mix(in srgb, CanvasText 60%, transparent); }
  .spacer { flex: 1; }
  .desc { margin: 6px 0 0; font-size: 12px; line-height: 1.4;
    color: color-mix(in srgb, CanvasText 72%, transparent); }
  .caps { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
  .cap { font-size: 10.5px; padding: 2px 7px; border-radius: 999px;
    background: color-mix(in srgb, CanvasText 8%, transparent);
    color: color-mix(in srgb, CanvasText 62%, transparent); }
  .cap.sensitive { background: color-mix(in srgb, #e0a800 20%, transparent);
    color: color-mix(in srgb, CanvasText 88%, transparent); font-weight: 600; }
  button {
    font-size: 12px; padding: 5px 12px; border-radius: 7px; cursor: pointer;
    border: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
    background: color-mix(in srgb, CanvasText 6%, transparent); color: CanvasText;
  }
  button:disabled { opacity: 0.5; cursor: default; }
  .mini { padding: 4px 10px; font-size: 11.5px; }
  .primary { background: color-mix(in srgb, #2f7bd6 90%, CanvasText); color: white;
    border-color: transparent; font-weight: 600; }
  .danger { color: #d24; border-color: color-mix(in srgb, #d24 40%, transparent); }
  .refresh { font-size: 12px; }
</style>
