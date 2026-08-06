<script lang="ts">
  import './styles/app.css'
  import './styles/editor-base.css'
  import 'mermaid-mini/style.css'
  import './styles/responsive.css'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { listen } from '@tauri-apps/api/event'
  import { invoke } from '@tauri-apps/api/core'
  import { onOpenUrl } from '@tauri-apps/plugin-deep-link'
  import TabBar from './components/TabBar.svelte'
  import EditorPane from './components/EditorPane.svelte'
  import EmptyState from './components/EmptyState.svelte'
  import ModeToggle from './components/ModeToggle.svelte'
  import { activeTab, tabs, closeTab, openFile, newFile, isDirty, activate } from './lib/tabs.svelte'
  import { createNewBase } from './lib/base/create'
  import { loadSettings, settings, removeRecentFile } from './lib/settings.svelte'
  import { loadLocale, t, i18n } from './lib/i18n/store.svelte'
  import { cmdOpen, cmdSave, cmdSaveAs, cmdPrint, cmdCloseActive, cmdToggleMode, dispatch, type CommandId } from './lib/commands'
  import { cmdMdblockRefresh } from './lib/mdblock/commands'
  import { setMirrorSourceResolver } from './lib/outline/store.svelte'
  import { confirmDirtyClose, showError } from './lib/dialogs'
  import { startAutoSaveWatcher } from './lib/autosave.svelte'
  import { installFocusPoll } from './lib/file-watcher.svelte'
  import SettingsDialog from './components/SettingsDialog.svelte'
  import UpdateBanner from './components/UpdateBanner.svelte'
  import UpdateDialog from './components/UpdateDialog.svelte'
  import Toast from './components/Toast.svelte'
  import FindReplace from './components/FindReplace.svelte'
  import { openFind, openFindReplace } from './lib/find-replace.svelte'
  import { buildContext, type TabSnapshot } from './lib/plugins/host'
  import { renderTabAsInlineBody, buildPdfTitle } from './lib/plugins/host-render-html'
  import { renderFilenameTemplate } from './lib/plugins/prompt'
  import {
    collectMenuItems, evaluateEnabled, parsePluginMenuId, CORE_MENU_ENABLED_ITEMS,
    type CollectedItem, type CollectedItems,
  } from './lib/plugins/menu-registry'
  import { pluginRuntime, setPluginDispatcher } from './lib/plugins/runtime.svelte'
  import { initActivePluginIds } from './lib/plugins/registry'
  import { getPluginScopedAll, pluginScopedVersion } from './lib/settings.svelte'
  import { pushToast } from './lib/toast.svelte'
  import type { PluginManifest, EnabledWhenContext, ToastLevel } from './lib/plugins/types'
  import { uiState, openSettings } from './lib/ui-state.svelte'
  import MobileToolbar from './components/MobileToolbar.svelte'
  import DrawerNav from './components/DrawerNav.svelte'
  import SidePanel from './components/side-panel/SidePanel.svelte'
  import {
    sidePanels, isSideVisible, loadSidePanels, registerBuiltinSideViews, toggleSideView, getSideView,
  } from './lib/side-panel/registry.svelte'
  import { loadFolderViewState, defaultRootToVault } from './lib/folder-view.svelte'
  import { loadOutlineGate } from './lib/outline/gate.svelte'
  import { loadHistoryGate, historyAppliesTo } from './lib/git-history/gate.svelte'
  import { loadOutlineDirs } from './lib/outline/dirs.svelte'
  import { platform, isIOS } from './lib/platform.svelte'
  import { vaultStore, refreshStatus, syncNow, attachStatusListener } from './lib/vault.svelte'
  import { canSyncActive, isTrackedVaultFile, deviceSourceForVaultPath, isMirroredSource, refreshSotvault, sotvaultStore, setVaultRootChangedHandler, initSotvaultNoteConflictToast } from './lib/sotvault.svelte'
  import { windowTitleFor } from './lib/window-title'
  import { installRecentsSync, refreshRecentMenu, mergedRecents } from './lib/recent-sync.svelte'
  import { maybeInstallTracker, shutdownTracker } from './lib/insights/tracker.svelte'
  import { ensureWikilinkBlocklist } from './lib/wikilink/blocklist-io.svelte'

  /** Open an in-memory read-only buffer received from the remote agent.
   *  The tab gets title "[remote] <basename>" and its content is pre-filled.
   *  No filePath → treated as untitled (won't auto-save or prompt for a path on
   *  first Cmd+S). read-only enforcement is a TODO: the Tab interface has no
   *  readOnly flag yet; for now the user CAN edit but the content is ephemeral.
   */
  function openRemoteBuffer(remotePath: string, content: string): void {
    const baseName = remotePath.split('/').pop() ?? remotePath
    // Re-use an already-open remote tab for the same path if present.
    const existing = tabs.find((t) => t.title === `[remote] ${baseName}` && t.filePath === '')
    if (existing) {
      existing.currentContent = content
      existing.initialContent = content
      activate(existing.id)
      return
    }
    const tab = {
      id: crypto.randomUUID(),
      filePath: '',
      title: `[remote] ${baseName}`,
      initialContent: content,
      currentContent: content,
      mode: 'source' as const,
      kind: 'markdown' as const,
      language: undefined,
      externalState: 'fresh' as const,
      externalBannerDismissed: false,
      lastKnownMtime: 0,
      lastKnownHash: '',
      pendingExternal: undefined,
    }
    tabs.push(tab)
    activate(tab.id)
  }

  let platformName = $state<'macos' | 'ios' | 'unknown'>('unknown')
  let drawerOpen = $state(false)
  $effect(() => {
    platform().then((p) => { platformName = p })
  })

  let showUpdateDialog = $state(false)
  let collectedItems = $derived<CollectedItems>(collectMenuItems(pluginRuntime.manifests))
  // Tracks last applied enabled state per menu-item id, so we only invoke the
  // Tauri command when something actually changes.
  const lastEnabledState = new Map<string, boolean>()
  // Bumped whenever the native menu is rebuilt (locale switch, plugin toggle,
  // daily-notes toggle). A rebuild resets every item to its build-time default,
  // so the enabled-sync effect must forget what it thinks it applied and
  // re-push. The effect reads this so bumping it forces a re-run.
  let menuRebuildTick = $state(0)
  function resyncMenuEnabled() {
    lastEnabledState.clear()
    menuRebuildTick++
  }

  // Keep the native menu bar in the user's language. Runs on mount (after the
  // locale hydrates) and on every switch; re-pushes recent files since the menu
  // rebuild resets that submenu. Best-effort — no native menu on iOS/web.
  $effect(() => {
    const locale = i18n.locale
    void (async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        await invoke('set_menu_locale', { locale })
        const { refreshRecentMenu } = await import('./lib/recent-sync.svelte')
        await refreshRecentMenu()
        // set_menu_locale rebuilt the menu → enabled states were reset to
        // build-time defaults. Drop the dedup cache so they get re-pushed.
        resyncMenuEnabled()
      } catch { /* no native menu on this platform */ }
    })()
  })

  onMount(() => {
    let stopAutoSave: (() => void) | undefined
    let dispatchPlugin: (pluginId: string, command: string) => Promise<void> = async () => {}

    const win = getCurrentWindow()

    // Register file-open listeners IMMEDIATELY (before any async work)
    // so we never miss events from Rust side.
    const unlistenOpenFile = listen<string>('open-file', async (e) => {
      try {
        await openFile(e.payload)
        win.show()
        win.setFocus()
      } catch (err) { console.warn('[App] open-file:', err); showError(String(err)) }
    })

    // Quick note: tray "New Markdown" item + system-wide Cmd+Ctrl+M both emit
    // `quick-note`. Create + open + focus a timestamped inbox file.
    const unlistenQuickNote = listen('quick-note', async () => {
      try {
        // Show + focus the window BEFORE creating the note: the editor mounts
        // and grabs focus on mount, which is dropped if the webview is still
        // hidden (app living in the tray). A visible window makes the mount-time
        // focus stick, so the note opens straight into edit state.
        await win.show()
        await win.setFocus()
        const { createQuickNote } = await import('./lib/quick-note.svelte')
        await createQuickNote()
      } catch (err) { console.warn('[App] quick-note:', err); showError(String(err)) }
    })

    // Vault-link resolution: chat window requests editor to focus + open a file.
    const unlistenOpenPath = listen<string>('editor://open-path', async (e) => {
      try {
        await openFile(e.payload)
        win.show()
        win.setFocus()
      } catch (err) { console.warn('[App] editor://open-path:', err); showError(String(err)) }
    })

    // Web-mode remote buffer: agent sends file content → open as untitled tab.
    const unlistenOpenRemoteBuffer = listen<{ remote_path: string; content: string }>(
      'editor://open-remote-buffer',
      (e) => {
        try {
          openRemoteBuffer(e.payload.remote_path, e.payload.content)
          win.show()
          win.setFocus()
        } catch (err) { console.warn('[App] editor://open-remote-buffer:', err) }
      },
    )

    // v2 plugin → frontend toast bridge (host_api.rs emits "plugin-toast").
    const unlistenPluginToast = listen<{ level: ToastLevel; message: string; detail?: string }>(
      'plugin-toast',
      (e) => { pushToast(e.payload) },
    )

    // Market install/uninstall/enable-toggle (子项目③) reconciles the runtime and
    // emits `plugins-changed`. Re-fetch manifests so the frontend menu-model +
    // dispatch data reflect the new installed/enabled set. `collectedItems` is
    // derived from `pluginRuntime.manifests`, so this reactively updates the app
    // menu bar, tab context menu, and settings tabs. NOTE: the native macOS menu
    // is built once at setup — a *newly installed* plugin's menu item may need a
    // restart to appear; enable/disable of already-present items reflects here.
    const unlistenPluginsChanged = listen('plugins-changed', async () => {
      try {
        pluginRuntime.manifests = await invoke<PluginManifest[]>('get_plugin_manifests')
      } catch (e) { console.warn('[App] plugins-changed refresh:', e) }
    })

    // A native menu rebuild (plugin install/toggle, daily-notes toggle, locale
    // switch) resets every item to its build-time enabled default. Drop the
    // enabled-sync dedup cache so the effect re-pushes the real runtime state.
    const unlistenMenuRebuilt = listen('menu-rebuilt', () => resyncMenuEnabled())

    invoke<string[]>('drain_pending_files').then(async (paths) => {
      for (const p of paths) {
        try { await openFile(p) } catch (err) { console.warn('[App] drain_pending_files:', err); showError(String(err)) }
      }
    }).catch((err) => console.warn('[App] drain_pending_files:', err))

    const unlistenDeepLink = onOpenUrl((urls) => {
      for (const url of urls) {
        let path = url
        if (path.startsWith('file://')) {
          try {
            const u = new URL(path)
            path = decodeURIComponent(u.pathname)
          } catch {
            // Fall through; openFile will reject if path is bad
          }
        }
        openFile(path).catch((err) => { console.warn('[App] deep-link openFile:', path, err); showError(String(err)) })
      }
    })

    ;(async () => {
      try { await loadSettings() } catch (e) { console.warn('[App] loadSettings:', e) }
      try { await loadLocale() } catch (e) { console.warn('[App] loadLocale:', e) }
      // Power Mode:配置从 settings 插件域读,并监听插件窗口经 RPC 推来的更新。
      try {
        await (await import('./lib/power-mode/host-config.svelte')).initPowerModeHost()
      } catch (e) { console.warn('[App] initPowerModeHost:', e) }
      await loadFolderViewState()
      await loadOutlineGate()
      await loadHistoryGate()
      // Register the built-in side views before the panels first render so the
      // registry is populated; loadSidePanels() then hydrates per-side visibility/
      // width/active tab (migrating legacy outline/history/folderView settings).
      registerBuiltinSideViews()
      await loadSidePanels()
      await loadOutlineDirs()
      await (await import('./lib/quick-note.svelte')).loadInboxDir()
      const { migrateMirrorMeta } = await import('./lib/sotvault.svelte')
      void migrateMirrorMeta()
      await initActivePluginIds()

      // Kick off auto-update check (1.5s delay built in, 20h cache).
      // Fire-and-forget — failures stay silent in the banner; Settings shows them.
      void (async () => {
        try {
          const { initUpdater } = await import('./lib/updater.svelte')
          await initUpdater()
        } catch (e) {
          console.warn('[App] updater init:', e)
        }
      })()
      try {
        const { installHoverInvalidator } = await import('./lib/mdblock-hover/hover-store.svelte')
        installHoverInvalidator()
      } catch (e) { console.warn('[App] installHoverInvalidator:', e) }
      // Theme initialization: load registry, install style slots, observe
      // system appearance, and keep activeTheme.id + slot CSS in sync.
      try {
        const { loadThemes, themes, findThemeById } = await import('./lib/themes.svelte')
        const { ensureThemeSlots, applyThemeContent, computeActiveThemeId, observePrefersColorScheme } = await import('./lib/theme-loader')
        const { setActiveTheme } = await import('./lib/active-theme.svelte')
        await loadThemes()
        ensureThemeSlots()

        let systemDark = false
        let lightAssigned: string | null = null
        let darkAssigned: string | null = null
        // followSystem is the third field of the host.theme.css bundle (it
        // decides whether the kit picks dark_css), so a change to it alone
        // must still reach plugin windows.
        let followAssigned: boolean | null = null

        // Plugin windows are isolated webviews: they never see the style slots
        // above, so an editor.kit plugin re-fetches host.theme.css on this
        // push. Fire-and-forget — it touches no reactive state, so it is safe
        // to call from inside the $effect below.
        function notifyPluginWindows() {
          void invoke('plugin_v2_theme_changed').catch(() => {})
        }

        async function syncSlots() {
          const t = settings.theme
          let changed = false
          if (t.light !== lightAssigned) {
            const meta = findThemeById(t.light)
            if (meta) { await applyThemeContent('light', meta.id) }
            lightAssigned = t.light
            changed = true
          }
          if (t.dark !== darkAssigned) {
            const meta = findThemeById(t.dark)
            if (meta) { await applyThemeContent('dark', meta.id) }
            darkAssigned = t.dark
            changed = true
          }
          if (t.followSystem !== followAssigned) {
            followAssigned = t.followSystem
            changed = true
          }
          setActiveTheme(computeActiveThemeId(t, systemDark))
          // One push per actual change (not per changed slot).
          if (changed) notifyPluginWindows()
        }

        const stopSystem = observePrefersColorScheme((dark) => {
          systemDark = dark
          void syncSlots()
        })
        // Re-sync whenever settings.theme changes (the dropdowns mutate it).
        const stopWatch = $effect.root(() => {
          $effect(() => {
            void settings.theme.light
            void settings.theme.dark
            void settings.theme.followSystem
            void syncSlots()
          })
        })
        // Also re-sync when themes list changes (import added new themes).
        const stopThemesWatch = $effect.root(() => {
          $effect(() => {
            void themes.list
            lightAssigned = null
            darkAssigned = null
            void syncSlots()
          })
        })
        ;(window as unknown as { __mdeditor_stop_theme?: () => void }).__mdeditor_stop_theme = () => {
          stopSystem()
          stopWatch()
          stopThemesWatch()
        }
      } catch (e) { console.warn('[App] theme init:', e) }
      stopAutoSave = startAutoSaveWatcher()

      if (await isIOS()) {
        pluginRuntime.manifests = []
        attachStatusListener()
        await refreshStatus()

        document.addEventListener('visibilitychange', () => {
          if (document.visibilityState === 'visible' && vaultStore.configured) {
            void syncNow()
          }
        })
      } else {
        try { pluginRuntime.manifests = await invoke<PluginManifest[]>('get_plugin_manifests') }
        catch (e) { console.warn('[App] get_plugin_manifests:', e) }
      }
      const manifestById: Record<string, PluginManifest> = Object.fromEntries(
        pluginRuntime.manifests.map((m) => [m.id, m]))

      // First-launch nudge: offer to install the `notemd` shell command.
      // Fire-and-forget so a slow dialog doesn't block the rest of startup.
      void (async () => {
        try {
          const { getCliPromptShown, setCliPromptShown } = await import('./lib/settings.svelte')
          if (await getCliPromptShown()) return
          const status = await invoke<{ installed: boolean; path: string | null }>('cli_install_status')
          if (status.installed) {
            // Already linked — mark prompt as resolved so we don't ask again.
            await setCliPromptShown(true)
            return
          }
          // Small delay so the main window finishes appearing before the dialog.
          await new Promise((r) => setTimeout(r, 1200))
          const { ask } = await import('@tauri-apps/plugin-dialog')
          const yes = await ask(
            t('cli.installPrompt'),
            { title: t('cli.installTitle'), kind: 'info' }
          )
          await setCliPromptShown(true)
          if (yes) {
            const candidates = await invoke<string[]>('cli_install_candidates')
            if (candidates.length > 0) {
              const dir = candidates[0]
              try {
                await invoke('cli_install', { dir })
                const { pushToast } = await import('./lib/toast.svelte')
                pushToast({ level: 'success', message: t('cli.installed', { dir }) })
              } catch (e) {
                const { pushToast } = await import('./lib/toast.svelte')
                pushToast({ level: 'error', message: t('cli.installFailed', { error: String(e) }) })
              }
            }
          }
        } catch (e) {
          console.warn('[App] cli first-launch prompt:', e)
        }
      })()

      dispatchPlugin = async (pluginId: string, command: string) => {
        // Side-view toggles: folder-view / outline-notes / git-history and any
        // future registered side view all route through the registry. The
        // per-side "one active tab" model replaces the old outline↔history
        // mutual-exclusion special-casing.
        if (command === 'toggle' && getSideView(pluginId)) {
          await toggleSideView(pluginId)
          return
        }
        // Custom-editor fixture: "New .cef fixture" → pick save path → create
        // empty file → openFile (routes via custom-editor registry → iframe tab).
        if (pluginId === 'notemd.cef-fixture') {
          if (command === 'create') {
            try {
              const { pickSaveFile } = await import('./lib/dialogs')
              const { writeTextFile } = await import('@tauri-apps/plugin-fs')
              const path = await pickSaveFile('untitled.cef')
              if (!path) return
              await writeTextFile(path, '')
              await openFile(path)
            } catch (e) { showError(String(e)) }
          }
          return
        }
        const m = manifestById[pluginId]
        if (!m) { console.warn('[App] unknown plugin', pluginId); return }
        const menu = m.menus?.find((me) => me.command === command)
        const tab = activeTab()
        const snap = {
          path: tab?.filePath ?? null,
          filename: tab?.title ?? null,
          extension: tab?.filePath?.split('.').pop() ?? null,
          kind: tab?.kind === 'image' ? 'markdown' : (tab?.kind ?? 'markdown'),
          title: tab ? buildPdfTitle(tab) : 'Untitled',
          isDirty: tab ? tab.currentContent !== tab.initialContent : false,
          isUntitled: !tab?.filePath,
          content: tab?.currentContent ?? '',
        }

        // If the menu item declares a save-dialog prompt, ask the user where
        // to write before invoking. Cancel → silent return.
        let outputPath: string | undefined
        if (menu?.prompt?.kind === 'save-dialog') {
          const { save } = await import('@tauri-apps/plugin-dialog')
          const defaultPath = renderFilenameTemplate(menu.prompt.default_filename, snap.path)
          const picked = await save({ defaultPath, filters: menu.prompt.filters })
          if (!picked) return
          outputPath = picked.endsWith('.pdf') || menu.prompt.filters[0]?.extensions[0] !== 'pdf'
            ? picked
            : `${picked}.pdf`
        }

        const htmlBaker = async (snapshot: TabSnapshot) => {
          const t = tabs.find((tab) => tab.filePath === snapshot.path)
          if (!t) throw new Error('renderer.html: no matching open tab')
          // Image tabs have no HTML body to render (defensive — no surviving
          // renderer.html plugin targets image tabs).
          if (t.kind === 'image') return ''
          // Plugins (md2pdf, future) take just the inline body and wrap
          // it themselves.
          return renderTabAsInlineBody(t)
        }

        // A command that maps to a plugin window opens that window instead of
        // executing on the process runtime (spec §7.2 — pure-UI plugins).
        const openWin = m.open_windows?.[command]
        if (openWin) {
          try {
            await invoke('plugin_v2_open_window', { pluginId: m.id, windowId: openWin })
          } catch (e) {
            pushToast({
              level: 'error',
              message: t('plugins.internalError', { name: m.name }),
              detail: String(e),
            })
          }
          return
        }
        // The command executes on the plugin's resident runtime process, which
        // reports back through the plugin-toast event listener rather than
        // returning anything to render here.
        try {
          const { context } = await buildContext(m, snap, { htmlBaker, outputPath })
          await invoke('plugin_v2_execute', { pluginId: m.id, command, context })
        } catch (e) {
          pushToast({
            level: 'error',
            message: t('plugins.internalError', { name: m.name }),
            detail: String(e),
          })
        }
      }

      // Make dispatchPlugin reachable from other components (TabBar's right-click).
      setPluginDispatcher(dispatchPlugin)
      // Populate the Open Recent menu now that settings (recentFiles) and the
      // vault root are loaded. installRecentsSync deliberately skips this — it
      // can run before loadSettings finishes.
      void refreshSotvault().then(() => refreshRecentMenu()).catch((e) => console.warn('[App] recents init:', e))
      void initSotvaultNoteConflictToast().catch((e) => console.warn('[App] note-conflict toast init:', e))
    })()

    const uninstallFocus = installFocusPoll()

    window.addEventListener('keydown', onKeyDown)

    let unlistenClose: (() => void) | Promise<() => void> | null = null
    ;(async () => {
      if (await isIOS()) return
      // Keep the window alive on close: hide instead of destroy so it can be
      // reopened from the dock. We MUST preventDefault here — without it the
      // webview's close proceeds and destroys the window (racing/overriding the
      // Rust-side prevent_close), after which the app has no window to re-show.
      unlistenClose = await win.onCloseRequested(async (event) => {
        event.preventDefault()
        while (tabs.length > 0) {
          await closeTab(tabs[0].id, async () => isDirty(tabs[0].id) ? 'save' : 'discard')
        }
        await win.hide()
      })
    })()

    let cleanupRecents: (() => void) | null = null
    installRecentsSync().then((fn) => { cleanupRecents = fn })

    // reading-insights tracker: install is STATE-driven, not boot-order-driven.
    // Register a handler so every refreshSotvault() that (re)loads the vault root
    // — at startup (post-plugin-init) or when the user configures a vault mid-
    // session — (re)installs the tracker idempotently. A direct call covers the
    // case where the root was already loaded before this handler was registered.
    setVaultRootChangedHandler(() => { void maybeInstallTracker(); void ensureWikilinkBlocklist(); void defaultRootToVault(sotvaultStore.vaultRoot) })
    // 镜像的伴生笔记要在 front-matter 里记下源文件(OKF §5.1)。store 不直接依赖
    // sotvault(会成环),所以解析器在这里注入。
    setMirrorSourceResolver((mainPath) => deviceSourceForVaultPath(mainPath))
    void maybeInstallTracker().catch((e) => console.warn('[App] insights tracker init:', e))
    void ensureWikilinkBlocklist().catch((e) => console.warn('[App] wikilink blocklist init:', e))
    // 空白启动默认把文件夹视图的根指向 vault(直连补一次:root 可能已在本处理器
    // 注册前就被 refreshSotvault 装载好了)。守卫:仅当尚无根时才设。
    void defaultRootToVault(sotvaultStore.vaultRoot).catch((e) => console.warn('[App] folder default root:', e))

    const unlistenMenu = listen<string>('menu-event', async (e) => {
      const id = e.payload
      const plugin = parsePluginMenuId(id)
      if (plugin) {
        await dispatchPlugin(plugin.pluginId, plugin.command)
        return
      }
      if (id.startsWith('open-recent:')) {
        const idx = parseInt(id.slice('open-recent:'.length), 10)
        const path = mergedRecents.paths[idx]
        if (path) {
          try {
            await openFile(path)
          } catch (e) {
            await removeRecentFile(path)
            await showError(String(e))
          }
        }
        return
      }
      switch (id) {
        case 'new':         newFile(); break
        case 'new-base':    await createNewBase(); break
        case 'open':        cmdOpen(); break
        case 'save':        cmdSave(); break
        case 'save-as':     cmdSaveAs(); break
        case 'print':       cmdPrint(); break
        case 'close-tab':   cmdCloseActive(); break
        case 'toggle-mode': cmdToggleMode(); break
        case 'find':        openFind(); break
        case 'find-replace': openFindReplace(); break
        // Custom item (not PredefinedMenuItem::select_all — see lib.rs): broadcast
        // to whichever editor is mounted, same convention as mdeditor:find-*.
        case 'select-all':  window.dispatchEvent(new CustomEvent('mdeditor:select-all')); break
        case 'zoom-in':     document.documentElement.style.fontSize = `${Math.min(200, (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16) + 2)}px`; break
        case 'zoom-out':    document.documentElement.style.fontSize = `${Math.max(10, (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16) - 2)}px`; break
        case 'zoom-reset':  document.documentElement.style.fontSize = ''; break
        case 'preferences': uiState.showSettings = true; break
        case 'check-for-updates': {
          // Open the dialog first so the user sees "checking…" feedback,
          // then trigger a forced refresh that bypasses the 20h cache.
          showUpdateDialog = true
          void (async () => {
            try {
              const { runCheck } = await import('./lib/updater.svelte')
              await runCheck({ forceFresh: true })
            } catch (e) {
              console.warn('[App] manual update check:', e)
            }
          })()
          break
        }
        case 'docs':
          import('@tauri-apps/plugin-opener')
            .then(({ openUrl }) => openUrl('https://github.com/wizlijun/note.md'))
            .catch(() => {})
          break
        case 'cli-install': {
          const { invoke } = await import('@tauri-apps/api/core')
          const { ask } = await import('@tauri-apps/plugin-dialog')
          const candidates = await invoke<string[]>('cli_install_candidates')
          // Walk candidates; first acceptance installs there.
          for (const dir of candidates) {
            const ok = await ask(t('cli.installInto', { dir }), { title: t('cli.installTitle'), kind: 'info' })
            if (ok) {
              try {
                await invoke('cli_install', { dir })
                const { pushToast } = await import('./lib/toast.svelte')
                pushToast({ level: 'success', message: t('cli.installed', { dir }) })
              } catch (e) {
                const { pushToast } = await import('./lib/toast.svelte')
                pushToast({ level: 'error', message: t('cli.installFailed', { error: String(e) }) })
              }
              break
            }
          }
          break
        }
        case 'cli-uninstall': {
          const { invoke } = await import('@tauri-apps/api/core')
          const status = await invoke<{ installed: boolean; path: string | null }>('cli_install_status')
          if (!status.installed || !status.path) {
            const { pushToast } = await import('./lib/toast.svelte')
            pushToast({ level: 'info', message: t('cli.notInstalled') })
            break
          }
          const dir = status.path.replace(/\/(?:notemd|mdedit)$/, '')
          try {
            await invoke('cli_uninstall', { dir })
            const { pushToast } = await import('./lib/toast.svelte')
            pushToast({ level: 'success', message: t('cli.uninstalled', { dir }) })
          } catch (e) {
            const { pushToast } = await import('./lib/toast.svelte')
            pushToast({ level: 'error', message: t('cli.uninstallFailed', { error: String(e) }) })
          }
          break
        }
        default:
          // Central command dispatcher: the PRIMARY path for all core menu ids
          // (share/unshare/copy-share-link, sync-to-vault, the three side-panel
          // toggles) plus the iOS-port commands.
          await dispatch(id as CommandId)
          break
      }
    })

    const unlistenDrop = win.onDragDropEvent(async (event) => {
      if (event.payload.type === 'drop') {
        for (const path of event.payload.paths) {
          if (path.toLowerCase().endsWith('.zip')) {
            try {
              const report = await invoke('theme_import', { zipPath: path })
              const { pendingThemeImport } = await import('./lib/theme-import-bus.svelte')
              pendingThemeImport.report = report
              uiState.showSettings = true   // surface the SettingsDialog so its child dialog renders
            } catch (e) { console.warn('[App] drop theme_import:', e) }
            continue
          }
          try { await openFile(path) } catch (e) { console.warn('[App] drop openFile:', e); showError(String(e)) }
        }
      }
    })

    return () => {
      window.removeEventListener('keydown', onKeyDown)
      uninstallFocus()
      if (unlistenClose) {
        if (typeof unlistenClose === 'function') unlistenClose()
        else unlistenClose.then((fn) => fn())
      }
      unlistenMenu.then((fn) => fn())
      unlistenDrop.then((fn) => fn())
      unlistenOpenFile.then((fn) => fn())
      unlistenQuickNote.then((fn) => fn())
      unlistenOpenPath.then((fn) => fn())
      unlistenOpenRemoteBuffer.then((fn) => fn())
      unlistenPluginToast.then((fn) => fn())
      unlistenPluginsChanged.then((fn) => fn())
      unlistenMenuRebuilt.then((fn) => fn())
      unlistenDeepLink.then((fn) => fn())
      cleanupRecents?.()
      setVaultRootChangedHandler(null)
      void shutdownTracker()
      stopAutoSave?.()
    }
  })

  function onKeyDown(e: KeyboardEvent) {
    if (!e.metaKey) return
    const k = e.key.toLowerCase()
    if (k === 'n' && !e.shiftKey) { e.preventDefault(); newFile() }
    else if (k === 'f' && !e.shiftKey) { e.preventDefault(); openFind() }
    else if (k === 'f' && e.shiftKey) { e.preventDefault(); openFindReplace() }
    else if (k === 'o') { e.preventDefault(); cmdOpen() }
    else if (k === 's' && !e.shiftKey) { e.preventDefault(); cmdSave() }
    else if (k === 's' && e.shiftKey) { e.preventDefault(); cmdSaveAs() }
    else if (k === 'w') { e.preventDefault(); cmdCloseActive() }
    else if (k === '/') { e.preventDefault(); cmdToggleMode() }
    else if (k === 'b' && e.shiftKey && settings.mdblock.enabled) {
      e.preventDefault()
      void cmdMdblockRefresh()
    }
  }

  let current = $derived(activeTab())

  // Whether the active mirror's this-device source file still exists on disk.
  // The "Reveal Sync Source" menu item is enabled ONLY when it does — matching
  // the SyncOriginBanner reveal affordance (revealing a missing file just
  // fails). Stays false until confirmed present, so it never wrongly enables.
  let syncSourceExists = $state(false)
  $effect(() => {
    const src = deviceSourceForVaultPath(current?.filePath || null)
    void sotvaultStore.tick
    syncSourceExists = false
    if (!src) return
    let cancelled = false
    import('@tauri-apps/plugin-fs')
      .then(({ exists }) => exists(src).catch(() => false))
      .then((ok) => { if (!cancelled) syncSourceExists = ok })
    return () => { cancelled = true }
  })

  // Right-edge inset for the floating mode toggle: push it left by the right
  // panel width whenever that side is showing, so it stays over the editor.
  let rightPanelOffset = $derived(isSideVisible('right', current) ? sidePanels.right.width : 0)

  // Window title: filename when single tab, plain "note.md" otherwise. A
  // vault-mirrored source gets the `↔` marker — with one tab there's no tab bar,
  // so this is the only place that says "you're editing the source".
  $effect(() => {
    const tabCount = tabs.length
    void sotvaultStore.tick
    const doc = tabCount === 1 && current ? current.title : null
    const title = windowTitleFor(doc, isMirroredSource(current?.filePath || null))
    getCurrentWindow().setTitle(title).catch(() => {})
  })

  // Re-evaluate enabled_when expressions whenever the active tab, its content
  // (for hasContent/isDirty), or plugin-scoped settings change. Pushes the
  // result to the native menu via the Tauri command. Deduped so we only
  // invoke when state actually flips.
  $effect(() => {
    const tab = current
    // Read these so the effect re-runs on tab/content changes:
    const _tabCount = tabs.length
    const _settingsTick = pluginScopedVersion.value
    void _tabCount; void _settingsTick
    const _sotvaultTick = sotvaultStore.tick
    void _sotvaultTick
    // Re-run (and re-push, since resyncMenuEnabled() cleared the cache) after a
    // native menu rebuild reset every item to its build-time enabled default.
    const _menuRebuildTick = menuRebuildTick
    void _menuRebuildTick

    const ewTab: EnabledWhenContext['currentTab'] = tab
      ? {
          path: tab.filePath || null,
          filename: tab.title || null,
          extension: tab.filePath ? (tab.filePath.split('.').pop() ?? null) : null,
          kind: tab.kind === 'image' ? null : tab.kind,
          hasContent: tab.kind === 'image' ? !!tab.filePath : (tab.currentContent ?? '').length > 0,
          isDirty: tab.currentContent !== tab.initialContent,
          isUntitled: !tab.filePath,
          canSyncToVault: canSyncActive(tab.filePath || null),
          isTrackedVaultFile: isTrackedVaultFile(tab.filePath || null),
          hasSyncSource: !!deviceSourceForVaultPath(tab.filePath || null) && syncSourceExists,
          isInVault: historyAppliesTo(tab, sotvaultStore.vaultRoot),
        }
      : null

    const allItems: CollectedItem[] = [
      ...collectedItems.file,
      ...collectedItems.edit,
      ...collectedItems.view,
      ...collectedItems.window,
      ...collectedItems.help,
      ...collectedItems.plugins,
      ...CORE_MENU_ENABLED_ITEMS,
    ]

    for (const item of allItems) {
      if (!item.enabledWhen) continue  // always-enabled — no need to invoke
      const ctx: EnabledWhenContext = {
        currentTab: ewTab,
        settings: getPluginScopedAll(item.pluginId),
        vaultConfigured: sotvaultStore.vaultRoot !== null,
      }
      const enabled = evaluateEnabled(item, ctx)
      if (lastEnabledState.get(item.id) === enabled) continue
      lastEnabledState.set(item.id, enabled)
      invoke('set_plugin_menu_item_enabled', { id: item.id, enabled })
        .catch((e) => console.warn(`[App] set_plugin_menu_item_enabled ${item.id}:`, e))
    }
  })
</script>

<main>
  {#if platformName === 'ios'}
    <MobileToolbar onOpenDrawer={() => (drawerOpen = true)} />
    <DrawerNav bind:open={drawerOpen} />
  {:else}
    <UpdateBanner onShowDetails={() => (showUpdateDialog = true)} />
  {/if}
  <TabBar />
  <Toast />
  <FindReplace />
  <section class="pane">
    {#if platformName !== 'ios'}
      <SidePanel side="left" tab={current ?? null} />
    {/if}
    {#if current}
      {#if tabs.length === 1 && platformName !== 'ios'}
        <div class="float-toggle" style="right: {rightPanelOffset + 28}px"><ModeToggle tab={current} /></div>
      {/if}
      <EditorPane tab={current} />
    {:else}
      <EmptyState />
    {/if}
    {#if platformName !== 'ios'}
      <SidePanel side="right" tab={current ?? null} />
    {/if}
  </section>
  <SettingsDialog bind:open={uiState.showSettings} />
  <UpdateDialog bind:open={showUpdateDialog} />
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    /* clip, not hidden: keep this from ever being a programmatically-scrollable
       container (focus()/scrollIntoView on a descendant would otherwise push the
       tab bar out of view with no way back). */
    overflow: clip;
  }
  .pane {
    position: relative;
    flex: 1;
    display: flex;
    overflow: hidden;
  }
  .pane :global(.empty),
  .pane :global(.src),
  .pane :global(.rich-wrap),
  .pane :global(.html-preview-wrap),
  .pane :global(.image-preview-wrap) {
    flex: 1;
    min-width: 0;
  }
  .float-toggle {
    position: absolute;
    top: 0;
    right: 28px;
    z-index: 10;
  }
</style>
