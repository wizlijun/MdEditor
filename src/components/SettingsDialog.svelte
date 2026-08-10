<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { onMount } from 'svelte'
  import { ask, open as openFilePicker } from '@tauri-apps/plugin-dialog'
  import { settings, saveSettings, getPluginScopedAll, mergePluginScoped, pluginScopedVersion } from '../lib/settings.svelte'
  import { i18n, setLocale, availableLocales, t, type Locale } from '../lib/i18n/store.svelte'
  import type { Messages } from '../lib/i18n/en'
  import { pluginTabLabel, pluginFieldLabel } from '../lib/plugins/plugin-i18n'
  import {
    updater as updaterState, runCheck as updaterRunCheck, setCheckOnStartup,
    downloadAndInstall as updaterDownloadAndInstall, restartApp as updaterRestart,
    hasUpdateForSettings,
  } from '../lib/updater.svelte'
  import { themes, loadThemes, reloadThemes } from '../lib/themes.svelte'
  import { pendingThemeImport } from '../lib/theme-import-bus.svelte'
  import ThemeImportDialog from './ThemeImportDialog.svelte'
  import { platform } from '../lib/platform.svelte'
  import { collectSettingsTabs, type SettingsTab } from '../lib/plugins/settings-registry'
  import { coreShareSettingsTab } from '../lib/share/settings-tab'
  import type { PluginManifest } from '../lib/plugins/types'
  import { isPluginActive } from '../lib/plugins/registry'
  import { outlineShortcuts, setShortcutOverride } from '../lib/outline/gate.svelte'
  import { outlineDirs, setOutlineDir } from '../lib/outline/dirs.svelte'
  import { inboxDir, setInboxDir } from '../lib/quick-note.svelte'
  import { vaultSettings, loadVaultSettings, saveSyncDir, DEFAULT_SYNC_DIR, saveLargeFileThreshold, DEFAULT_LARGE_FILE_THRESHOLD_MB, saveSearchExcludeDirs } from '../lib/vault-settings.svelte'
  import { pushToast } from '../lib/toast.svelte'
  import {
    DEFAULT_SHORTCUTS, resolveShortcuts, displayShortcut, eventToShortcut, findConflict,
    type OutlineCommandId,
  } from '../lib/outline/shortcuts'
  import VaultSettingsTab from './VaultSettingsTab.svelte'

  let { open = $bindable(false) }: { open: boolean } = $props()

  let isIOSPlatform = $state(false)
  $effect(() => {
    platform().then((p) => { isIOSPlatform = p === 'ios' })
  })

  let pluginTabs = $state<SettingsTab[]>([])
  let selectedTab = $state<'core' | string>('core')
  let pluginValues = $state<Record<string, Record<string, unknown>>>({})

  // Vault-scoped sync folder (stored in {vault}/.notemd/settings.json). Loaded
  // whenever the dialog opens; edited into a draft, then saved on demand.
  let syncDirDraft = $state('')
  let syncDirBusy = $state(false)
  let thresholdDraft = $state(DEFAULT_LARGE_FILE_THRESHOLD_MB)
  let thresholdBusy = $state(false)
  let searchExcludeDirsDraft = $state('')
  let searchExcludeDirsBusy = $state(false)
  $effect(() => {
    if (!open) return
    void loadVaultSettings().then(() => {
      syncDirDraft = vaultSettings.syncDir
      thresholdDraft = vaultSettings.largeFileThresholdMb
      searchExcludeDirsDraft = vaultSettings.searchExcludeDirs.join('\n')
    })
  })
  async function onSetOutlineDir(kind: 'wikipage' | 'dailynote', value: string) {
    try {
      await setOutlineDir(kind, value)
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    }
  }
  async function onSetInboxDir(value: string) {
    try {
      await setInboxDir(value)
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    }
  }
  async function onSaveSyncDir() {
    syncDirBusy = true
    try {
      await saveSyncDir(syncDirDraft)
      syncDirDraft = vaultSettings.syncDir
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      syncDirBusy = false
    }
  }
  async function onSaveThreshold() {
    const mb = Math.max(1, Math.floor(Number(thresholdDraft) || DEFAULT_LARGE_FILE_THRESHOLD_MB))
    thresholdBusy = true
    try {
      await saveLargeFileThreshold(mb)
      thresholdDraft = vaultSettings.largeFileThresholdMb
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      thresholdBusy = false
    }
  }
  async function onSaveSearchExcludeDirs() {
    const dirs = searchExcludeDirsDraft
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
    searchExcludeDirsBusy = true
    try {
      await saveSearchExcludeDirs(dirs)
      searchExcludeDirsDraft = vaultSettings.searchExcludeDirs.join('\n')
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      searchExcludeDirsBusy = false
    }
  }

  type CliStatus = { installed: boolean; path: string | null; target_valid: boolean }
  let cliStatus = $state<CliStatus | null>(null)
  let cliBusy = $state(false)
  let cliError = $state<string | null>(null)

  let updaterBusy = $state(false)
  let updaterMessage = $state<string | null>(null)

  async function handleCheckUpdate() {
    updaterBusy = true
    updaterMessage = null
    try {
      await updaterRunCheck({ forceFresh: true })
      if (updaterState.state === 'uptodate') {
        updaterMessage = t('settings.update.upToDate')
      } else if (updaterState.state === 'available') {
        updaterMessage = t('settings.update.foundNew', { version: updaterState.latestVersion ?? '' })
      }
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    } finally {
      updaterBusy = false
    }
  }

  async function handleUpdateNow() {
    updaterBusy = true
    updaterMessage = null
    try {
      await updaterDownloadAndInstall()
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    } finally {
      updaterBusy = false
    }
  }

  async function handleRestart() {
    try {
      await updaterRestart()
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    }
  }

  async function onCheckOnStartupToggle(e: Event) {
    await setCheckOnStartup((e.currentTarget as HTMLInputElement).checked)
  }

  function formatLastChecked(iso: string | null): string {
    if (!iso) return t('time.never')
    const ms = Date.parse(iso)
    if (!Number.isFinite(ms)) return iso
    const d = new Date(ms)
    return d.toLocaleString()
  }

  async function refreshCliStatus() {
    try {
      cliStatus = await invoke<CliStatus>('cli_install_status')
    } catch (e) {
      console.warn('[SettingsDialog] cli_install_status:', e)
      cliStatus = { installed: false, path: null, target_valid: false }
    }
  }

  async function handleCliInstall() {
    cliBusy = true
    cliError = null
    try {
      const candidates = await invoke<string[]>('cli_install_candidates')
      const { ask } = await import('@tauri-apps/plugin-dialog')
      for (const dir of candidates) {
        const ok = await ask(`Install 'notemd' into ${dir}?`, {
          title: "Install 'notemd' Command",
          kind: 'info',
        })
        if (ok) {
          await invoke('cli_install', { dir })
          break
        }
      }
    } catch (e) {
      cliError = e instanceof Error ? e.message : String(e)
    } finally {
      cliBusy = false
      await refreshCliStatus()
    }
  }

  async function handleCliUninstall() {
    if (!cliStatus?.installed || !cliStatus.path) return
    cliBusy = true
    cliError = null
    try {
      const dir = cliStatus.path.replace(/\/(?:notemd|mdedit)$/, '')
      await invoke('cli_uninstall', { dir })
    } catch (e) {
      cliError = e instanceof Error ? e.message : String(e)
    } finally {
      cliBusy = false
      await refreshCliStatus()
    }
  }

  onMount(async () => {
    try {
      const manifests = await invoke<PluginManifest[]>('get_plugin_manifests')
      // Defensive guard: no shipped manifest contributes a share settings tab
      // anymore (share is core), but filter regardless so a stray external
      // 'share' plugin can't duplicate the core Share tab.
      pluginTabs = [coreShareSettingsTab(), ...collectSettingsTabs(manifests.filter((m) => m.id !== 'share'))]
    } catch (e) {
      console.warn('[SettingsDialog] manifest load:', e)
      pluginTabs = [coreShareSettingsTab()]
    }
    void refreshCliStatus()
  })

  $effect(() => {
    void pluginScopedVersion.value
    if (!open || pluginTabs.length === 0) return
    for (const tab of pluginTabs) {
      const all = getPluginScopedAll(tab.pluginId)
      const stripped: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(all)) {
        stripped[k.slice(tab.pluginId.length + 1)] = v
      }
      pluginValues[tab.pluginId] = stripped
    }
  })

  async function savePluginField(pluginId: string, key: string, value: unknown) {
    pluginValues[pluginId] = { ...(pluginValues[pluginId] ?? {}), [key]: value }
    await mergePluginScoped({ [`${pluginId}.${key}`]: value })
  }

  // The 22 categories cover every extension our editor supports as a document type.
  // These must match the `fileAssociations` in src-tauri/tauri.conf.json — that's
  // what tells macOS that note.md is a legitimate handler for these UTIs in the first place.
  const FILE_GROUPS: { label: string; exts: string[] }[] = [
    { label: 'Markdown',      exts: ['md', 'markdown', 'mdown', 'mkd'] },
    // NOTE: intentionally NOT registering html/htm here. Setting note.md as the
    // default handler for public.html makes macOS treat it as a web-browser
    // candidate and lets it hijack the default browser. Passive "Open With"
    // support (Info.plist) stays, but we never claim the default role.
    // 只剩 Markdown + 纯文本。此前还登记了 20 类代码/配置扩展名(py/rs/go/
    // sql/…),让 note.md 挤进每个源码文件的「打开方式」——对代码文件是打扰
    // 不是价值。要用 note.md 开这些文件,拖进窗口或「打开方式 → 其他」仍可以。
    { label: 'Plain text',    exts: ['txt', 'text'] },
  ]
  const ALL_EXTS = FILE_GROUPS.flatMap((g) => g.exts)

  type ExtResult = {
    ext: string
    uti: string | null
    ok: boolean
    error: string | null
  }

  let busy = $state(false)
  let resultText = $state<string | null>(null)
  let resultDetails = $state<ExtResult[] | null>(null)

  async function handleSetDefault() {
    const ok = await ask(
      `This will register note.md as the macOS default application for ${ALL_EXTS.length} ` +
        `file extensions across ${FILE_GROUPS.length} categories.\n\n` +
        `From then on, double-clicking any of these file types in Finder will open them in note.md instead of your current default editor.\n\n` +
        `Categories: ${FILE_GROUPS.map((g) => g.label).join(', ')}.\n\n` +
        `You can revert this for any single type later: in Finder, select a file → Get Info → "Open with" → choose another app → click "Change All…".\n\n` +
        `Continue?`,
      {
        title: 'Set note.md as default for text & code files',
        kind: 'warning',
        okLabel: 'Set as default',
        cancelLabel: 'Cancel',
      },
    )
    if (!ok) return
    busy = true
    resultText = null
    resultDetails = null
    try {
      const results = await invoke<ExtResult[]>('set_default_app_for_extensions', {
        exts: ALL_EXTS,
      })
      resultDetails = results
      const successes = results.filter((r) => r.ok).length
      const failures = results.filter((r) => !r.ok)
      if (failures.length === 0) {
        resultText = t('settings.defaultApp.resultOk', { count: successes })
      } else {
        resultText = t('settings.defaultApp.resultPartial', {
          ok: successes,
          total: results.length,
          failed: failures.map((f) => `.${f.ext}`).join(', '),
        })
      }
    } catch (e) {
      resultText = t('settings.defaultApp.resultError', { error: e instanceof Error ? e.message : String(e) })
    } finally {
      busy = false
    }
  }

  async function onToggle(e: Event) {
    settings.autoSave = (e.currentTarget as HTMLInputElement).checked
    await saveSettings()
  }

  let importReport = $state<unknown | null>(null)
  let importBusy = $state(false)

  // ── git proxy ────────────────────────────────────────────────────────────
  // Machine-local, so it lives in shared.json rather than the vault's own
  // settings (which sync). Loaded lazily the first time the dialog opens.
  let gitProxyInput = $state('')
  let gitProxyBusy = $state(false)
  let gitProxyError = $state<string | null>(null)
  let gitProxySaved = $state(false)
  let gitProxyLoaded = false

  $effect(() => {
    if (!open || gitProxyLoaded || isIOSPlatform) return
    gitProxyLoaded = true
    void (async () => {
      const { getGitProxy } = await import('../lib/shared-config')
      gitProxyInput = await getGitProxy().catch(() => '')
    })()
  })

  async function onSaveGitProxy() {
    gitProxyBusy = true
    gitProxyError = null
    gitProxySaved = false
    try {
      const { setGitProxy } = await import('../lib/shared-config')
      // The host returns the normalized value, so the field shows exactly what
      // was stored (trimmed) rather than what was typed.
      gitProxyInput = await setGitProxy(gitProxyInput)
      gitProxySaved = true
    } catch (e) {
      // Surfaced verbatim: the host writes these to be read by a human
      // ("unsupported proxy scheme 'ftp' — use http, https, socks5 or socks5h").
      gitProxyError = typeof e === 'string' ? e : String(e)
    } finally {
      gitProxyBusy = false
    }
  }

  // Mirror the App.svelte drag-drop bus: when a .zip is dropped, App stuffs
  // the prepared ImportReport into pendingThemeImport.report; we surface it
  // here so the ThemeImportDialog modal renders.
  $effect(() => {
    if (pendingThemeImport.report) {
      importReport = pendingThemeImport.report
      pendingThemeImport.report = null
    }
  })

  async function onLightThemeChange(e: Event) {
    settings.theme.light = (e.currentTarget as HTMLSelectElement).value
    await saveSettings()
  }
  async function onDarkThemeChange(e: Event) {
    settings.theme.dark = (e.currentTarget as HTMLSelectElement).value
    await saveSettings()
  }
  async function onFollowSystemToggle(e: Event) {
    settings.theme.followSystem = !(e.currentTarget as HTMLInputElement).checked
    // Note: the *checkbox label* says "Always use light theme", so
    // checked means !followSystem.
    await saveSettings()
  }

  async function handleImportTheme() {
    const selection = await openFilePicker({
      multiple: false,
      directory: false,
      filters: [{ name: 'Typora theme zip', extensions: ['zip'] }],
    })
    if (!selection || Array.isArray(selection)) return
    importBusy = true
    try {
      importReport = await invoke('theme_import', { zipPath: selection })
    } catch (e) {
      console.warn('[Settings] theme_import:', e)
      importReport = { themes: [], asset_dirs: [], errors: [{ file: '?', message: String(e) }], staging_dir: '' }
    } finally {
      importBusy = false
    }
  }

  async function handleRevealThemes() {
    try { await invoke('theme_reveal') }
    catch (e) { console.warn('[Settings] theme_reveal:', e) }
  }

  async function handleReloadThemes() {
    await reloadThemes()
  }

  async function handleRestoreBuiltins() {
    try { await invoke('theme_restore_builtins') }
    catch (e) { console.warn('[Settings] theme_restore_builtins:', e) }
    await reloadThemes()
  }

  $effect(() => { void loadThemes() })

  // Outline shortcut rebinding
  const isMac = typeof navigator !== 'undefined' && navigator.platform.includes('Mac')
  let recording = $state<OutlineCommandId | null>(null)
  let conflictMsg = $state('')
  let resolvedOutline = $derived(resolveShortcuts(outlineShortcuts.overrides))

  const OUTLINE_CMD_LABELS: Record<OutlineCommandId, keyof Messages> = {
    'outline.indent': 'outline.cmd.indent',
    'outline.outdent': 'outline.cmd.outdent',
    'outline.toggleCollapse': 'outline.cmd.toggleCollapse',
    'outline.moveUp': 'outline.cmd.moveUp',
    'outline.moveDown': 'outline.cmd.moveDown',
    'outline.bold': 'outline.cmd.bold',
    'outline.italic': 'outline.cmd.italic',
  }

  async function onRecordKey(e: KeyboardEvent, id: OutlineCommandId) {
    e.preventDefault(); e.stopPropagation()
    if (e.key === 'Escape') { recording = null; conflictMsg = ''; return }
    const sc = eventToShortcut(e)
    if (!sc) return
    const trial = resolveShortcuts({ ...outlineShortcuts.overrides, [id]: sc })
    const conflict = findConflict(trial, id)
    if (conflict) { conflictMsg = t('outline.shortcutConflict', { other: t(OUTLINE_CMD_LABELS[conflict]) }); return }
    conflictMsg = ''
    await setShortcutOverride(id, sc)
    recording = null
  }
</script>

{#if open}
  <div
    class="overlay"
    role="presentation"
    onclick={() => (open = false)}
    onkeydown={(e) => e.key === 'Escape' && (open = false)}
  >
    <div class="dialog" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
      <h2>{t('settings.title')}</h2>

      <nav class="tab-strip">
        <button class:active={selectedTab === 'core'} onclick={() => selectedTab = 'core'}>{t('settings.tab.core')}</button>
        <button class:active={selectedTab === 'block'} onclick={() => selectedTab = 'block'}>{t('settings.tab.block')}</button>
        {#if !isIOSPlatform}
          <button class:active={selectedTab === 'cli'} onclick={() => { selectedTab = 'cli'; void refreshCliStatus() }}>{t('settings.tab.cli')}</button>
          <button class:active={selectedTab === 'updates'} onclick={() => { selectedTab = 'updates' }}>{t('settings.tab.updates')}</button>
        {/if}
        {#if isIOSPlatform}
          <button class:active={selectedTab === 'vault'} onclick={() => selectedTab = 'vault'}>{t('settings.tab.vault')}</button>
        {/if}
        <button class:active={selectedTab === 'outline-notes'} onclick={() => selectedTab = 'outline-notes'}>{t('settings.tab.outline')}</button>
        {#each pluginTabs as ptab (ptab.pluginId)}
          <button class:active={selectedTab === ptab.pluginId} onclick={() => selectedTab = ptab.pluginId}>{pluginTabLabel(ptab.manifest, ptab.label)}</button>
        {/each}
      </nav>

      {#if selectedTab === 'core'}
        <section class="block">
          <label class="row">
            <span class="lbl">{t('settings.language')}</span>
            <select value={i18n.locale} onchange={(e) => setLocale((e.currentTarget as HTMLSelectElement).value as Locale)}>
              {#each availableLocales as loc (loc.code)}
                <option value={loc.code}>{loc.label}</option>
              {/each}
            </select>
          </label>
        </section>

        <section class="block">
          <h3>{t('settings.themes')}</h3>
          <label class="row">
            <span class="lbl">{t('settings.lightTheme')}</span>
            <select value={settings.theme.light} onchange={onLightThemeChange}>
              {#each themes.list as th (th.id)}
                <option value={th.id}>{th.name}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('settings.darkTheme')}</span>
            <select value={settings.theme.dark} onchange={onDarkThemeChange}>
              {#each themes.list as th (th.id)}
                <option value={th.id}>{th.name}</option>
              {/each}
            </select>
          </label>
          <label class="row" style="margin-top: 6px;">
            <input
              type="checkbox"
              checked={!settings.theme.followSystem}
              onchange={onFollowSystemToggle}
            />
            {t('settings.alwaysLight')}
          </label>
          <div class="row" style="gap: 8px; flex-wrap: wrap; margin-top: 8px;">
            <button onclick={handleImportTheme} disabled={importBusy}>
              {importBusy ? t('themeImport.importing') : t('settings.importTypora')}
            </button>
            <button onclick={handleRevealThemes}>{t('settings.revealThemes')}</button>
            <button onclick={handleReloadThemes}>{t('settings.reloadThemes')}</button>
            <button onclick={handleRestoreBuiltins}>{t('settings.restoreBuiltins')}</button>
          </div>
          {#if themes.error}
            <p class="desc" style="color: tomato;">{t('settings.themesLoadFailed', { error: themes.error })}</p>
          {/if}
        </section>

        {#if importReport}
          <ThemeImportDialog
            report={importReport as never}
            onClose={() => { importReport = null; reloadThemes() }}
          />
        {/if}

        <section class="block">
          <label class="row">
            <input type="checkbox" checked={settings.autoSave} onchange={onToggle} />
            {t('settings.autoSaveLabel')}
          </label>
          {#if !isIOSPlatform}
            <label class="row">
              <input
                type="checkbox"
                bind:checked={settings.dailyNotes.enabled}
                onchange={async () => {
                  await saveSettings()
                  try {
                    const { invoke } = await import('@tauri-apps/api/core')
                    await invoke('set_daily_notes_enabled', { enabled: settings.dailyNotes.enabled })
                  } catch (e) { console.warn('[settings] set_daily_notes_enabled:', e) }
                }}
              />
              {t('settings.dailyNotes.label')}
            </label>
          {/if}
        </section>

        <section class="block">
          <h3>{t('vaultSync.title')}</h3>
          <p class="desc">{t('vaultSync.desc')}</p>
          <label class="row">
            <span class="lbl">{t('vaultSync.vaultPath')}</span>
            <input type="text" readonly
              value={vaultSettings.vaultPath ?? t('vaultSync.notConfigured')} />
          </label>
          <label class="row">
            <span class="lbl">{t('vaultSync.relPath')}</span>
            <input type="text" bind:value={syncDirDraft}
              placeholder={DEFAULT_SYNC_DIR}
              disabled={!vaultSettings.vaultPath || syncDirBusy} />
            <button onclick={onSaveSyncDir}
              disabled={!vaultSettings.vaultPath || syncDirBusy}>{t('vaultSync.save')}</button>
          </label>
          <label class="row">
            <span class="lbl">{t('vaultSync.largeFileThreshold')}</span>
            <input type="number" min="1" step="1" bind:value={thresholdDraft}
              disabled={!vaultSettings.vaultPath || thresholdBusy} />
            <button onclick={onSaveThreshold}
              disabled={!vaultSettings.vaultPath || thresholdBusy}>{t('vaultSync.save')}</button>
          </label>
          <label class="row" style="align-items: flex-start;">
            <span class="lbl">{t('settings.searchExcludeDirs')}</span>
            <div style="flex: 1; display: flex; flex-direction: column; gap: 4px;">
              <textarea rows="3" bind:value={searchExcludeDirsDraft}
                disabled={!vaultSettings.vaultPath || searchExcludeDirsBusy}
                style="width: 100%; font: inherit; padding: 6px; border-radius: 4px; border: 1px solid color-mix(in srgb, CanvasText 25%, transparent); background: Canvas; color: CanvasText; resize: vertical;"
              ></textarea>
              <p class="desc" style="margin: 0;">{t('settings.searchExcludeDirsHint')}</p>
              <button onclick={onSaveSearchExcludeDirs}
                style="align-self: flex-start;"
                disabled={!vaultSettings.vaultPath || searchExcludeDirsBusy}>{t('vaultSync.save')}</button>
            </div>
          </label>
        </section>

        {#if !isIOSPlatform}
          <section class="block">
            <h3>{t('settings.defaultApp.heading')}</h3>
            <p class="desc">{@html t('settings.defaultApp.desc1')}</p>
            <p class="desc">{@html t('settings.defaultApp.desc2', { exts: ALL_EXTS.length, groups: FILE_GROUPS.length })}</p>
            <details class="ext-list">
              <summary>{t('settings.defaultApp.showTypes', { count: ALL_EXTS.length })}</summary>
              <ul>
                {#each FILE_GROUPS as g}
                  <li><strong>{g.label}</strong> — {g.exts.map((e) => `.${e}`).join(', ')}</li>
                {/each}
              </ul>
            </details>
            <button class="primary" onclick={handleSetDefault} disabled={busy}>
              {busy ? t('settings.defaultApp.setting') : t('settings.defaultApp.setDefault', { count: ALL_EXTS.length })}
            </button>
            {#if resultText}
              <p
                class="result"
                class:ok={resultDetails?.every((r) => r.ok)}
                class:partial={resultDetails && resultDetails.some((r) => !r.ok) && resultDetails.some((r) => r.ok)}
                class:fail={resultDetails && resultDetails.every((r) => !r.ok)}
              >
                {resultText}
              </p>
            {/if}
            <p class="undo-note">{@html t('settings.defaultApp.undoNote')}</p>
          </section>

          <section class="block">
            <h3>{t('settings.proxy.title')}</h3>
            <p class="desc">{t('settings.proxy.desc')}</p>
            <div class="row" style="gap: 8px;">
              <input
                id="git-proxy"
                type="text"
                style="flex: 1;"
                placeholder="http://127.0.0.1:1080"
                bind:value={gitProxyInput}
                onkeydown={(e) => { if (e.key === 'Enter') void onSaveGitProxy() }}
              />
              <button onclick={() => void onSaveGitProxy()} disabled={gitProxyBusy}>
                {gitProxyBusy ? t('settings.proxy.saving') : t('settings.proxy.save')}
              </button>
            </div>
            {#if gitProxyError}
              <p class="result fail">{gitProxyError}</p>
            {:else if gitProxySaved}
              <p class="result ok">{t('settings.proxy.saved')}</p>
            {/if}
            <p class="undo-note">{t('settings.proxy.sshNote')}</p>
          </section>
        {/if}
      {:else if selectedTab === 'block'}
        <section class="block">
          <label class="row">
            <input type="checkbox" bind:checked={settings.mdblock.enabled} onchange={() => saveSettings()} />
            {t('settings.block.enable')}
          </label>
          <p class="desc">{@html t('settings.block.enableDesc')}</p>
        </section>

        <section class="block">
          <p class="desc">{@html t('settings.block.savingDesc')}</p>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.injectAiHint}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {@html t('settings.block.injectHint')}
          </label>
        </section>

        <section class="block">
          <h3>{t('settings.chunk.heading')}</h3>
          <label class="row">
            <span class="lbl">{t('settings.chunk.strategy')}</span>
            <select bind:value={settings.mdblock.chunkStrategy}
                    disabled={!settings.mdblock.enabled}
                    onchange={() => saveSettings()}>
              <option value="section">{t('settings.chunk.sectionFirst')}</option>
              <option value="size">{t('settings.chunk.sizeFirst')}</option>
            </select>
          </label>
          <p class="desc">{@html t('settings.chunk.sectionDesc')}</p>
          <label class="row" style:opacity={settings.mdblock.chunkStrategy === 'section' ? 1 : 0.5}>
            <span class="lbl">{t('settings.chunk.sectionCutLevel')}</span>
            <select bind:value={settings.mdblock.sectionCutLevel}
                    disabled={!settings.mdblock.enabled || settings.mdblock.chunkStrategy !== 'section'}
                    onchange={() => saveSettings()}>
              <option value={1}>{t('settings.chunk.h1opt')}</option>
              <option value={2}>{t('settings.chunk.h2opt')}</option>
              <option value={3}>{t('settings.chunk.h3opt')}</option>
            </select>
          </label>
          <label class="row" style:opacity={settings.mdblock.chunkStrategy === 'section' ? 1 : 0.5}>
            <span class="lbl">{t('settings.chunk.minChars')}</span>
            <input type="number" min="0" max="5000" step="50"
                   bind:value={settings.mdblock.sectionMinChars}
                   disabled={!settings.mdblock.enabled || settings.mdblock.chunkStrategy !== 'section'}
                   onchange={() => saveSettings()} />
          </label>
          <label class="row">
            <span class="lbl">{t('settings.chunk.maxChars')}</span>
            <input type="number" min="200" max="20000" step="100"
                   bind:value={settings.mdblock.chunkSizeChars}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
          </label>
          <p class="desc">{t('settings.chunk.maxCharsDesc')}</p>
          <label class="row">
            <span class="lbl">{t('settings.chunk.similarity')}</span>
            <input type="number" min="0" max="1" step="0.05"
                   bind:value={settings.mdblock.similarityThreshold}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
          </label>
        </section>

        <p class="desc">{@html t('settings.chunk.affectNote')}</p>

        <section class="block">
          <h3>{t('settings.viz.heading')}</h3>
          <p class="desc">{t('settings.viz.desc')}</p>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.hover.showSourceGutter}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {t('settings.viz.sourceMarkers')}
          </label>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.hover.showRichOverlay}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {t('settings.viz.richGutter')}
          </label>
        </section>
      {:else if selectedTab === 'cli'}
        <section class="block">
          <h3>{t('settings.cli.heading')}</h3>
          <p class="desc">{@html t('settings.cli.desc')}</p>
          {#if cliStatus === null}
            <p class="desc">{t('settings.cli.loading')}</p>
          {:else if cliStatus.installed}
            <p class="desc">
              {t('settings.cli.installedAtLabel')} <code>{cliStatus.path}</code>
              {#if !cliStatus.target_valid}
                <br />
                <span style="color: tomato;">{t('settings.cli.symlinkMismatch')}</span>
              {/if}
            </p>
            <div class="row" style="gap: 8px; flex-wrap: wrap;">
              <button onclick={handleCliInstall} disabled={cliBusy}>
                {cliBusy ? t('settings.cli.working') : t('settings.cli.reinstall')}
              </button>
              <button onclick={handleCliUninstall} disabled={cliBusy}>
                {cliBusy ? t('settings.cli.working') : t('settings.cli.uninstall')}
              </button>
            </div>
          {:else}
            <p class="desc">{t('settings.cli.notInstalled')}</p>
            <button class="primary" onclick={handleCliInstall} disabled={cliBusy}>
              {cliBusy ? t('settings.cli.installing') : t('settings.cli.install')}
            </button>
          {/if}
          {#if cliError}
            <p class="result fail">{t('settings.cli.error', { error: cliError })}</p>
          {/if}
          <p class="desc" style="margin-top: 12px;">{@html t('settings.cli.helpDesc')}</p>
        </section>
      {:else if selectedTab === 'updates'}
        <section class="block">
          <h3>{t('settings.update.heading')}</h3>
          <p class="desc">
            {t('settings.update.currentVersionLabel')}<strong>v{updaterState.currentVersion || '—'}</strong>
          </p>
          <p class="desc">
            {t('settings.update.lastChecked', { time: formatLastChecked(updaterState.lastCheckedAt) })}
          </p>
          <label class="row" style="margin-top: 8px;">
            <input type="checkbox" checked={updaterState.checkOnStartup} onchange={onCheckOnStartupToggle} />
            {t('settings.update.autoCheck')}
          </label>
          <div class="row" style="gap: 8px; flex-wrap: wrap; margin-top: 10px;">
            <button onclick={handleCheckUpdate} disabled={updaterBusy || updaterState.state === 'checking' || updaterState.state === 'downloading'}>
              {updaterState.state === 'checking' ? t('settings.update.checking') : t('settings.update.checkNow')}
            </button>
            {#if hasUpdateForSettings() && updaterState.state !== 'downloading' && updaterState.state !== 'ready'}
              <button class="primary" onclick={handleUpdateNow} disabled={updaterBusy}>
                {t('settings.update.downloadInstall', { version: updaterState.latestVersion ?? '' })}
              </button>
            {/if}
            {#if updaterState.state === 'ready'}
              <button class="primary" onclick={handleRestart}>{t('settings.update.restartNow')}</button>
            {/if}
          </div>

          {#if updaterState.state === 'downloading'}
            <p class="desc" style="margin-top: 10px;">
              {t('settings.update.downloading')}
              {#if updaterState.contentLength}
                {Math.round((updaterState.downloaded / updaterState.contentLength) * 100)}%
              {:else}
                {(updaterState.downloaded / 1024 / 1024).toFixed(1)} MB
              {/if}
            </p>
          {/if}

          {#if updaterMessage}
            <p class="result" class:ok={updaterState.state === 'uptodate'} class:fail={updaterState.state === 'error'}>
              {updaterMessage}
            </p>
          {/if}

          {#if updaterState.state === 'available' && updaterState.notes}
            <details style="margin-top: 12px;">
              <summary>{t('settings.update.notes', { version: updaterState.latestVersion ?? '' })}</summary>
              <pre style="margin-top: 8px; white-space: pre-wrap; word-break: break-word; background: color-mix(in srgb, CanvasText 5%, transparent); padding: 8px; border-radius: 6px; font-size: 11px;">{updaterState.notes}</pre>
            </details>
          {/if}

          <p class="desc" style="margin-top: 14px;">
            {t('settings.update.distNote')}
          </p>
        </section>
      {:else if selectedTab === 'vault' && isIOSPlatform}
        <VaultSettingsTab />
      {:else if selectedTab === 'outline-notes'}
        <section class="block">
          <h3>{t('outline.shortcutsTitle')}</h3>
          {#each Object.keys(DEFAULT_SHORTCUTS) as id (id)}
            <div class="shortcut-row">
              <span class="shortcut-label">{t(OUTLINE_CMD_LABELS[id as OutlineCommandId])}</span>
              <button
                class="shortcut-input" class:recording={recording === id}
                onclick={() => (recording = id as OutlineCommandId)}
                onkeydown={(e) => recording === id && onRecordKey(e, id as OutlineCommandId)}
                onblur={() => { if (recording === id) recording = null }}
              >
                {recording === id ? t('outline.pressKeys') : displayShortcut(resolvedOutline[id as OutlineCommandId], isMac)}
              </button>
              {#if outlineShortcuts.overrides[id as OutlineCommandId]}
                <button class="shortcut-reset" onclick={() => void setShortcutOverride(id as OutlineCommandId, null)}>↺</button>
              {/if}
            </div>
          {/each}
          {#if conflictMsg}<p class="shortcut-conflict">{conflictMsg}</p>{/if}
        </section>
        <section class="block">
          <h3>{t('outline.dirsTitle')}</h3>
          <div class="field-row">
            <label for="wikipage-dir">{t('outline.wikipageDir')}</label>
            <input id="wikipage-dir" type="text" value={outlineDirs.wikipage}
              onchange={(e) => void onSetOutlineDir('wikipage', (e.currentTarget as HTMLInputElement).value)} />
          </div>
          <div class="field-row">
            <label for="dailynote-dir">{t('outline.dailynoteDir')}</label>
            <input id="dailynote-dir" type="text" value={outlineDirs.dailynote}
              onchange={(e) => void onSetOutlineDir('dailynote', (e.currentTarget as HTMLInputElement).value)} />
          </div>
          <div class="field-row">
            <label for="inbox-dir">{t('quickNote.inboxDir')}</label>
            <input id="inbox-dir" type="text" value={inboxDir.value}
              onchange={(e) => void onSetInboxDir((e.currentTarget as HTMLInputElement).value)} />
          </div>
        </section>
      {:else}
        {#each pluginTabs as ptab (ptab.pluginId)}
          {#if selectedTab === ptab.pluginId}
            <div class="plugin-settings">
              {#each ptab.schema as field (field.key)}
                {@const localKey = field.key.slice(ptab.pluginId.length + 1)}
                <label class="plugin-field">
                  <span class="lbl">{pluginFieldLabel(ptab.manifest, field.key, field.label)}</span>
                  {#if field.type === 'string'}
                    <input type="text"
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? field.default ?? ''}
                      placeholder={field.placeholder ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).value)} />
                  {:else if field.type === 'secret'}
                    <input type="password"
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).value)} />
                  {:else if field.type === 'select'}
                    <select
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? field.default ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLSelectElement).value)}>
                      {#each field.options as opt}
                        <option value={opt}>{opt}</option>
                      {/each}
                    </select>
                  {:else if field.type === 'boolean'}
                    <input type="checkbox"
                      checked={(pluginValues[ptab.pluginId]?.[localKey] as boolean) ?? field.default ?? false}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).checked)} />
                  {/if}
                </label>
              {/each}
            </div>
          {/if}
        {/each}
      {/if}

      <div class="actions">
        <button onclick={() => (open = false)}>{t('settings.done')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed; inset: 0;
    background: rgba(0,0,0,0.3);
    display: flex; align-items: center; justify-content: center;
    z-index: 100;
  }
  .dialog {
    width: min(560px, 92vw);
    max-height: 86vh;
    overflow: auto;
    background: Canvas;
    color: CanvasText;
    border: 1px solid color-mix(in srgb, CanvasText 20%, transparent);
    border-radius: 8px;
    padding: 18px 20px;
    box-shadow: 0 12px 40px rgba(0,0,0,0.25);
  }
  h2 { margin: 0 0 12px 0; font-size: 16px; }
  h3 {
    margin: 0 0 8px 0;
    font-size: 13px;
    font-weight: 600;
  }
  .block {
    padding: 12px 0;
    border-top: 1px solid color-mix(in srgb, CanvasText 10%, transparent);
  }
  .block:first-of-type { border-top: 0; padding-top: 0; }
  .row { display: flex; gap: 8px; align-items: center; font-size: 13px; }
  .row .lbl {
    width: 60px;
    flex-shrink: 0;
  }
  .row select {
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 13px;
    flex: 1;
    max-width: 240px;
  }
  .desc {
    font-size: 12px;
    line-height: 1.5;
    margin: 0 0 8px 0;
    color: color-mix(in srgb, CanvasText 75%, transparent);
  }
  .ext-list {
    margin: 8px 0;
    font-size: 12px;
  }
  .ext-list summary {
    cursor: pointer;
    color: color-mix(in srgb, CanvasText 65%, transparent);
    user-select: none;
  }
  .ext-list ul {
    margin: 8px 0 0 0;
    padding-left: 18px;
    line-height: 1.7;
    color: color-mix(in srgb, CanvasText 80%, transparent);
  }
  .ext-list li { font-family: ui-monospace, Menlo, monospace; font-size: 11px; }
  .ext-list li strong { font-family: -apple-system, system-ui, sans-serif; font-size: 12px; }
  .actions { display: flex; justify-content: flex-end; margin-top: 16px; }
  button {
    padding: 6px 14px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    cursor: pointer;
    font-size: 12px;
  }
  button:disabled { opacity: 0.5; cursor: progress; }
  .primary {
    background: AccentColor;
    color: AccentColorText;
    border-color: AccentColor;
    font-weight: 500;
  }
  .result {
    margin: 10px 0 0 0;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    line-height: 1.45;
    background: color-mix(in srgb, CanvasText 8%, transparent);
  }
  .result.ok {
    background: color-mix(in srgb, #2ea043 18%, transparent);
    color: color-mix(in srgb, #2ea043 90%, CanvasText);
  }
  .result.partial {
    background: color-mix(in srgb, #d29922 22%, transparent);
  }
  .result.fail {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }
  .undo-note {
    margin: 10px 0 0 0;
    font-size: 11px;
    line-height: 1.5;
    color: color-mix(in srgb, CanvasText 60%, transparent);
  }
  .tab-strip {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 20%, transparent);
    margin-bottom: 12px;
  }
  .tab-strip button {
    background: transparent;
    border: 0;
    padding: 8px 12px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    font-size: 12px;
    color: CanvasText;
    border-radius: 0;
  }
  .tab-strip button.active {
    border-bottom-color: AccentColor;
    font-weight: 600;
  }
  .plugin-settings { display: flex; flex-direction: column; gap: 12px; padding: 4px 0; }
  .plugin-field { display: flex; align-items: center; gap: 12px; font-size: 13px; }
  .plugin-field .lbl { width: 160px; flex-shrink: 0; }
  .plugin-field input[type="text"],
  .plugin-field input[type="password"],
  .plugin-field select {
    flex: 1;
    padding: 6px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 12px;
  }
  .shortcut-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 13px;
  }
  .shortcut-label {
    width: 120px;
    flex-shrink: 0;
  }
  .shortcut-input {
    font-family: ui-monospace, Menlo, monospace;
    font-size: 12px;
    padding: 4px 8px;
    min-width: 120px;
    text-align: center;
  }
  .shortcut-input.recording {
    outline: 1px solid AccentColor;
    color: AccentColor;
  }
  .shortcut-reset {
    padding: 4px 8px;
    font-size: 13px;
    opacity: 0.6;
  }
  .shortcut-reset:hover { opacity: 1; }
  .field-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 13px;
  }
  .field-row label {
    width: 140px;
    flex-shrink: 0;
  }
  .field-row input[type="text"] {
    flex: 1;
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 12px;
    max-width: 200px;
  }
  .shortcut-conflict {
    color: #d44a4a;
    font-size: 12px;
    margin: 4px 0 0 0;
  }
</style>
