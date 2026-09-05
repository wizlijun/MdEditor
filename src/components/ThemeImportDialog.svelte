<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { t } from '../lib/i18n/store.svelte'
  import { modalFocus } from '../lib/ui/modal-focus'
  import { pushToast } from '../lib/toast.svelte'

  interface ImportTheme {
    id: string
    name: string
    appearance: 'light' | 'dark'
    source_file: string
    conflict: boolean
  }
  interface ImportError { file: string; message: string }
  interface ImportReport {
    themes: ImportTheme[]
    asset_dirs: string[]
    errors: ImportError[]
    staging_dir: string
  }

  let { report, onClose }: { report: ImportReport; onClose: () => void } = $props()

  let overwrite = $state(false)
  let busy = $state(false)
  let operationError = $state<string | null>(null)

  const hasConflict = $derived(report.themes.some((t) => t.conflict))
  const canImport = $derived(!busy && report.themes.length > 0 && (!hasConflict || overwrite))

  async function confirm() {
    if (!canImport) return
    busy = true
    operationError = null
    try {
      const n = await invoke<number>('theme_install', { report, overwrite })
      console.info('[ThemeImport] installed', n, 'themes')
      onClose()
    } catch (e) {
      operationError = String(e)
    } finally {
      busy = false
    }
  }

  async function cancel() {
    if (busy) return
    busy = true
    operationError = null
    try {
      await invoke('theme_cancel_import', { stagingDir: report.staging_dir })
    } catch (e) {
      pushToast({ level: 'error', message: t('themeImport.errors'), detail: String(e) })
    } finally { busy = false; onClose() }
  }
</script>

<div class="overlay" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) void cancel() }}>
  <div class="dialog ui-surface" role="dialog" aria-modal="true" aria-labelledby="theme-import-title" aria-busy={busy} tabindex="-1"
    use:modalFocus={{ onClose: () => { void cancel() }, canClose: () => !busy }}>
    <h2 id="theme-import-title">{t('themeImport.title')}</h2>
    <div class="import-content">

    {#if report.themes.length === 0}
      <p>{t('themeImport.noneFound')}</p>
    {:else}
      <p>{t('themeImport.detected', { count: report.themes.length })}</p>
      <ul>
        {#each report.themes as th (th.id)}
          <li>
            <strong>{th.name}</strong> ({th.appearance})
            {#if th.conflict}<span class="warn">{t('themeImport.willOverwrite')}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}

    {#if report.asset_dirs.length > 0}
      <p>{t('themeImport.assetFolders')}</p>
      <ul>
        {#each report.asset_dirs as d (d)}<li>{d}</li>{/each}
      </ul>
    {/if}

    {#if report.errors.length > 0}
      <p>{t('themeImport.errors')}</p>
      <ul>
        {#each report.errors as e (e.file)}
          <li class="err">{e.file}: {e.message}</li>
        {/each}
      </ul>
    {/if}

    {#if hasConflict}
      <label class="overwrite">
        <input type="checkbox" checked={overwrite} onchange={(e) => overwrite = (e.currentTarget as HTMLInputElement).checked} />
        {t('themeImport.overwriteExisting')}
      </label>
    {/if}

    {#if operationError}<p class="operation-error" role="alert">{t('themeImport.errors')} {operationError}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={cancel} disabled={busy}>{t('common.cancel')}</button>
      <button class="primary" onclick={confirm} disabled={!canImport}>
        {busy ? t('themeImport.importing') : t('themeImport.import')}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; padding: 16px; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 200; }
  .dialog { display: flex; flex-direction: column; background: var(--ui-surface, Canvas); color: CanvasText; border: 1px solid var(--ui-separator); border-radius: 12px; width: min(560px, 100%); min-width: 0; max-height: calc(100dvh - 32px); overflow: hidden; box-shadow: 0 16px 48px #0004; }
  .dialog h2 { margin: 0; padding: 18px 22px; border-bottom: 1px solid var(--ui-separator); font-size: 17px; flex-shrink: 0; }
  .import-content { padding: 16px 22px; min-height: 0; overflow: auto; overscroll-behavior: contain; overflow-wrap: anywhere; }
  .dialog ul { margin: 6px 0 12px; padding-left: 1.4em; }
  .warn { color: var(--ui-warning); margin-left: 6px; }
  .err, .operation-error { color: var(--ui-danger); }
  .operation-error { padding: 10px 12px; background: var(--ui-bg); border-radius: 6px; font-size: 13px; }
  .overwrite { display: flex; gap: 6px; align-items: center; margin: 10px 0; }
  .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; padding: 12px 22px; border-top: 1px solid var(--ui-separator); flex-shrink: 0; }
  button { font: inherit; padding: 7px 14px; min-height: 32px; border-radius: 6px; border: 1px solid var(--ui-control-border); background: var(--ui-surface, Canvas); color: CanvasText; cursor: pointer; }
  button:not(:disabled):hover { background: var(--ui-hover); }
  button:disabled { opacity: 0.5; cursor: default; }
  .primary { font-weight: 600; background: var(--ui-accent); color: var(--ui-accent-foreground, white); border-color: var(--ui-accent); }
  button.primary:not(:disabled):hover { background: color-mix(in srgb, var(--ui-accent) 88%, black); }
</style>
