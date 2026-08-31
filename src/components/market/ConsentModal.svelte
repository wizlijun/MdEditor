<!-- src/components/market/ConsentModal.svelte — install-time capability consent
     (子项目③ / ②安全评审 V1). Before installing, `plugin_market_preview` runs
     the FULL verify pipeline (download → sha256 → minisign) on a throwaway copy
     and returns the verified manifest. We render that manifest's `capabilities`
     (a Vec<String> on ManifestV2 — NOT host_capabilities) so the user consents
     to exactly what passed verification. Only on "Trust & Install" do we call
     `plugin_market_install`. -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { i18n, t } from '../../lib/i18n/store.svelte'
  import { localizedPluginDescription, localizedPluginName } from '../../lib/market/plugin-text'
  import { capabilityLabel, isSensitiveCapability, type PluginMarketI18n } from '../../lib/market/types'

  interface Props {
    id: string
    version: string
    name: string
    description?: string | null
    i18n?: PluginMarketI18n | null
    /** Resolves after a successful install so the parent can re-fetch lists. */
    onInstalled: () => void
    onClose: () => void
  }
  let { id, version, name, description, i18n: registryI18n, onInstalled, onClose }: Props = $props()

  // Preview manifest returned by plugin_market_preview (verified, not installed).
  interface PreviewManifest {
    id: string
    name: string
    version: string
    description?: string | null
    i18n?: PluginMarketI18n | null
    capabilities?: string[]
  }

  let loading = $state(true)
  let installing = $state(false)
  let error = $state<string | null>(null)
  let manifest = $state<PreviewManifest | null>(null)
  let textSource = $derived({
    id,
    name: name || manifest?.name,
    description: description ?? manifest?.description,
    i18n: registryI18n ?? manifest?.i18n,
  })
  let displayName = $derived(localizedPluginName(textSource, i18n.locale))
  let displayDescription = $derived(localizedPluginDescription(textSource, i18n.locale))

  // Live download progress for THIS plugin, pushed by the host as bytes land
  // (`plugin-download-progress`). A multi-megabyte package over a slow link
  // otherwise looks like a hang.
  let received = $state(0)
  let total = $state<number | null>(null)

  function formatBytes(n: number): string {
    if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
    if (n >= 1024) return `${Math.round(n / 1024)} KB`
    return `${n} B`
  }

  let progressText = $derived(
    total != null
      ? t('pluginMarket.downloading', {
          done: formatBytes(received),
          total: formatBytes(total),
        })
      : t('pluginMarket.downloadingUnknown', { done: formatBytes(received) }),
  )

  $effect(() => {
    // `listen` resolves to an unlisten fn; the cleanup awaits it so the
    // subscription is dropped when the modal closes.
    const pending = listen<{ id: string; received: number; total: number | null }>(
      'plugin-download-progress',
      (e) => {
        if (e.payload.id !== id) return
        received = e.payload.received
        total = e.payload.total ?? null
      },
    )
    return () => {
      void pending.then((un) => un())
    }
  })

  // Capabilities sorted so sensitive ones surface first.
  let caps = $derived<string[]>(
    [...(manifest?.capabilities ?? [])].sort((a, b) => {
      const sa = isSensitiveCapability(a) ? 0 : 1
      const sb = isSensitiveCapability(b) ? 0 : 1
      return sa - sb
    }),
  )

  $effect(() => {
    void preview()
  })

  async function preview() {
    loading = true
    error = null
    try {
      manifest = await invoke<PreviewManifest>('plugin_market_preview', { id, version })
    } catch (e) {
      error = String(e)
    } finally {
      loading = false
    }
  }

  async function confirmInstall() {
    installing = true
    error = null
    try {
      await invoke('plugin_market_install', { id, version })
      onInstalled()
    } catch (e) {
      error = String(e)
      installing = false
    }
  }
</script>

<div class="overlay" role="presentation" onclick={() => !installing && onClose()}
     onkeydown={(e) => e.key === 'Escape' && !installing && onClose()}>
  <div class="modal" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
    <h2>{t('pluginMarket.consent.title', { name: displayName })}</h2>
    <p class="ver">{id} · {version}</p>

    {#if loading}
      <p class="msg">{t('pluginMarket.consent.verifying')}</p>
      {#if received > 0}
        <div class="progress">
          <div
            class="bar"
            class:indeterminate={total == null}
            style={total != null ? `width:${Math.min(100, (received / total) * 100)}%` : ''}
          ></div>
        </div>
        <p class="progress-text">{progressText}</p>
      {/if}
    {:else if error}
      <p class="msg error">{error}</p>
    {:else}
      {#if displayDescription}
        <p class="desc">{displayDescription}</p>
      {/if}
      <p class="intro">{t('pluginMarket.consent.intro')}</p>
      {#if caps.length === 0}
        <p class="none">{t('pluginMarket.consent.none')}</p>
      {:else}
        <ul class="caps">
          {#each caps as cap (cap)}
            <li class:sensitive={isSensitiveCapability(cap)}>
              <span class="dot" aria-hidden="true"></span>
              <span class="label">{capabilityLabel(cap)}</span>
              {#if isSensitiveCapability(cap)}
                <span class="warn-tag">{t('pluginMarket.consent.sensitive')}</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    {#if installing && received > 0}
      <div class="progress">
        <div
          class="bar"
          class:indeterminate={total == null}
          style={total != null ? `width:${Math.min(100, (received / total) * 100)}%` : ''}
        ></div>
      </div>
      <p class="progress-text">{progressText}</p>
    {/if}

    <div class="actions">
      <button class="ghost" onclick={onClose} disabled={installing}>{t('pluginMarket.cancel')}</button>
      <button class="primary" onclick={confirmInstall} disabled={loading || installing || !!error}>
        {installing ? t('pluginMarket.installing') : t('pluginMarket.consent.trustInstall')}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed; inset: 0; z-index: 100;
    background: color-mix(in srgb, CanvasText 40%, transparent);
    display: flex; align-items: center; justify-content: center; padding: 20px;
  }
  .modal {
    background: Canvas; color: CanvasText;
    border: 1px solid color-mix(in srgb, CanvasText 18%, transparent);
    border-radius: 12px; padding: 20px 22px; width: min(460px, 100%);
    max-height: 82vh; overflow: auto; box-shadow: 0 12px 40px rgba(0,0,0,0.28);
  }
  h2 { margin: 0 0 2px; font-size: 15px; }
  .ver { margin: 0 0 12px; font-size: 11px; font-family: ui-monospace, monospace;
    color: color-mix(in srgb, CanvasText 55%, transparent); }
  .desc { margin: 0 0 10px; font-size: 12px; line-height: 1.45;
    color: color-mix(in srgb, CanvasText 78%, transparent); }
  .intro { margin: 0 0 8px; font-size: 12px;
    color: color-mix(in srgb, CanvasText 70%, transparent); }
  .msg { font-size: 13px; padding: 10px 0; }
  .msg.error { color: #d24; }
  .progress {
    height: 4px; border-radius: 2px; overflow: hidden;
    background: color-mix(in srgb, CanvasText 12%, transparent);
  }
  .progress .bar {
    height: 100%; border-radius: 2px;
    background: color-mix(in srgb, AccentColor 85%, CanvasText);
    transition: width 0.2s ease;
  }
  /* No Content-Length (chunked response): sweep instead of pretending to know. */
  .progress .bar.indeterminate {
    width: 35%;
    animation: sweep 1.1s ease-in-out infinite;
  }
  @keyframes sweep {
    0% { margin-left: -35%; }
    100% { margin-left: 100%; }
  }
  .progress-text {
    margin: 5px 0 0; font-size: 11px;
    color: color-mix(in srgb, CanvasText 55%, transparent);
  }
  .none { font-size: 12px; color: color-mix(in srgb, CanvasText 55%, transparent); }
  .caps { list-style: none; margin: 0 0 4px; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .caps li {
    display: flex; align-items: center; gap: 8px; font-size: 12.5px;
    padding: 6px 8px; border-radius: 6px;
    background: color-mix(in srgb, CanvasText 5%, transparent);
  }
  .caps li.sensitive {
    background: color-mix(in srgb, #e0a800 16%, transparent);
    color: color-mix(in srgb, CanvasText 92%, transparent);
  }
  .dot { width: 6px; height: 6px; border-radius: 50%;
    background: color-mix(in srgb, CanvasText 45%, transparent); flex: 0 0 auto; }
  .caps li.sensitive .dot { background: #e0a800; }
  .label { flex: 1; }
  .warn-tag {
    font-size: 10px; text-transform: uppercase; letter-spacing: 0.4px;
    padding: 1px 6px; border-radius: 999px; font-weight: 600;
    background: #e0a800; color: #1a1400;
  }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  button {
    font-size: 12.5px; padding: 6px 14px; border-radius: 7px; cursor: pointer;
    border: 1px solid color-mix(in srgb, CanvasText 20%, transparent);
    background: color-mix(in srgb, CanvasText 6%, transparent); color: CanvasText;
  }
  button:disabled { opacity: 0.5; cursor: default; }
  .primary {
    background: color-mix(in srgb, #2f7bd6 90%, CanvasText); color: white;
    border-color: transparent; font-weight: 600;
  }
</style>
