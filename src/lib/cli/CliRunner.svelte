<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { buildContext } from '../plugins/host'
  import { renderTabAsInlineBody } from '../plugins/host-render-html'
  import { mkdir, writeTextFile } from '@tauri-apps/plugin-fs'
  import { getPluginScopedAll, loadSettings } from '../settings.svelte'
  import { generateInsightsReport } from '../insights/run'
  import { presetRange, type Preset } from '../insights/value'
  import { localTzOffsetMinutes } from '../insights/model'
  import { runShareCli, buildVirtualTab } from './share-cli'
  import { type CliPayload } from './cli-runner'
  import type { PluginManifest, TabKind } from '../plugins/types'
  import type { FileKind } from '../fs'

  /** Map FileKind to the narrower TabKind used in the plugin request context.
   *  Image tabs are reported as 'markdown' (matches App.svelte's snapshot
   *  builder); image files never reach a plugin's HTML pipeline — share
   *  handles them via uploadImage before this mapping matters. */
  function toTabKind(k: FileKind): TabKind {
    if (k === 'image') return 'markdown'
    return k
  }

  /** `notemd reading-insights report` — file-less; generates the digest (owner +
   *  online audience) and writes it to <vault>/stat or prints to stdout. */
  async function runInsightsReport(payload: CliPayload): Promise<void> {
    try {
      const vaultFlag = (payload.flags['vault'] as string | undefined) || undefined
      const vaultRoot = vaultFlag ?? (await invoke<string | null>('sotvault_vault_root'))
      if (!vaultRoot) {
        await finish({ exit_code: 2, stderr: ['notemd: no Vault configured. Pass --vault <path> or configure one in the app.'] })
        return
      }
      let from = payload.flags['from'] as string | undefined
      let to = payload.flags['to'] as string | undefined
      if (!from || !to) {
        const valid = ['today', 'yesterday', '7d', '30d', 'month']
        const dateFlag = payload.flags['date'] as string | undefined
        if (dateFlag && !valid.includes(dateFlag)) {
          await finish({ exit_code: 2, stderr: [`notemd: invalid --date preset '${dateFlag}'. Valid: ${valid.join(', ')}`] })
          return
        }
        const preset = (dateFlag ?? 'yesterday') as Preset
        const r = presetRange(preset, Date.now(), localTzOffsetMinutes())
        from = r.from
        to = r.to
      }
      const { filename, markdown } = await generateInsightsReport(from, to, vaultRoot)
      if (payload.flags['stdout']) {
        await finish({ exit_code: 0, stdout: markdown, stderr: [] })
        return
      }
      const base = vaultRoot.replace(/\/$/, '')
      await mkdir(`${base}/stat`, { recursive: true }).catch(() => {})
      const abs = `${base}/${filename}`
      await writeTextFile(abs, markdown)
      await finish({ exit_code: 0, stdout: `wrote ${abs}`, stderr: [] })
    } catch (e) {
      await finish({ exit_code: 1, stderr: [`notemd: reading-insights report failed: ${e}`] })
    }
  }

  async function run(): Promise<void> {
    let payload: CliPayload
    try {
      payload = await invoke<CliPayload>('cli_payload')
    } catch (e) {
      await finish({ exit_code: 1, stderr: [`notemd: failed to fetch cli payload: ${e}`] })
      return
    }

    // Hydrate the in-memory settings store from disk BEFORE any plugin
    // action emits `settings.merge`. Without this, the runner sees defaults
    // for every key, and the first save (e.g., updating share.records) wipes
    // the user's stored apiKey / baseUrl / plugins.enabled / recentFiles.
    try {
      await loadSettings()
    } catch (e) {
      await finish({ exit_code: 1, stderr: [`notemd: failed to load settings: ${e}`] })
      return
    }

    // reading-insights report: a file-less command that reuses the in-app report
    // logic (owner analytics from the Vault + audience stats fetched online with
    // the configured share API key + records). No plugin binary involved.
    if (payload.plugin_id === 'reading-insights') {
      await runInsightsReport(payload)
      return
    }

    // share 是 core：走 TS 实现，无插件二进制。复用与菜单一致的
    // vault-home 前置与 bake 流程；结果按 --json/--quiet 约定输出。
    // 契约与实现在 share-cli.ts（可单测），此处只注入真实 finish。
    if (payload.plugin_id === 'share') {
      await runShareCli(payload, { finish })
      return
    }

    const manifests = await invoke<PluginManifest[]>('get_plugin_manifests')
    const manifest = manifests.find(m => m.id === payload.plugin_id)
    if (!manifest) {
      const isV2 = payload.plugin_id.includes('.')
      await finish({ exit_code: 3, stderr: [
        isV2
          ? `notemd: v2 plugin '${payload.plugin_id}' is not installed or the v2 runtime flag is off.`
          : `notemd: plugin '${payload.plugin_id}' is not enabled. Run 'notemd plugin enable ${payload.plugin_id}'.`,
      ]})
      return
    }
    if (!payload.file) {
      await finish({ exit_code: 2, stderr: ['notemd: missing file argument'] })
      return
    }

    const built = await buildVirtualTab(payload.file, finish)
    if (!built) return
    const { tab: virtualTab, extension, fileKind } = built

    // For commands requiring rendered HTML, bake the content.
    const entry = (manifest.cli ?? []).find(c => c.subcommand === payload.subcommand)
    let renderedHtml: string | undefined
    if (entry?.requires_tab_context && manifest.host_capabilities.includes('renderer.html')) {
      try {
        renderedHtml = fileKind === 'image' ? '' : await renderTabAsInlineBody(virtualTab)
      } catch (e) {
        await finish({ exit_code: 1, stderr: [`notemd: render failed: ${e}`] })
        return
      }
    }

    // Resolve output_path for plugins that need it (e.g. md2pdf export).
    let outputPath: string | undefined
    const outputFlag = payload.flags['output'] as string | undefined
    if (outputFlag) {
      outputPath = outputFlag.startsWith('/') ? outputFlag
        : `${payload.file!.replace(/\/[^/]+$/, '')}/${outputFlag}`
    } else if (manifest.id === 'md2pdf' || manifest.id === 'notemd.md2pdf') {
      outputPath = payload.file!.replace(/\.[^.]+$/, '.pdf')
    }

    const pluginSettings = getPluginScopedAll(manifest.id)

    const snap = {
      path: virtualTab.filePath,
      filename: virtualTab.title,
      extension,
      kind: toTabKind(virtualTab.kind),
      title: virtualTab.title,
      isDirty: false,
      isUntitled: false,
      content: virtualTab.currentContent,
    }
    const invokeOpts = {
      htmlBaker: renderedHtml != null ? async () => renderedHtml! : undefined,
      settingsReader: () => pluginSettings,
      outputPath,
    }

    // The command executes on the plugin's resident runtime via
    // plugin_v2_execute, which returns a result value (toasts are GUI-only
    // events). Output conventions: --json wraps the result as {ok:true,data},
    // errors exit 4 with a plugin_failed envelope.
    try {
      const { context } = await buildContext(manifest, snap, invokeOpts)
      const data = await invoke<unknown>('plugin_v2_execute', {
        pluginId: manifest.id,
        command: payload.plugin_command,
        context,
      })
      const path = data != null && typeof data === 'object'
        ? (data as Record<string, unknown>).path : undefined
      await finish({
        exit_code: 0,
        stdout: payload.global.json
          ? JSON.stringify({ ok: true, data: data ?? {} })
          : typeof path === 'string' ? path : JSON.stringify(data ?? {}),
        stderr: [],
      })
    } catch (e) {
      const message = String(e)
      await finish({
        exit_code: 4,
        stdout: payload.global.json
          ? JSON.stringify({ ok: false, error: { code: 'plugin_failed', message } })
          : undefined,
        stderr: [`✗ ${manifest.name}: ${message}`],
      })
    }
  }

  async function finish(r: { exit_code: number; stdout?: string; stderr: string[] }): Promise<void> {
    try {
      await invoke('cli_finish', { result: r })
    } catch (e) {
      console.error('[cli] cli_finish failed:', e)
    }
  }

  onMount(() => {
    run().catch(async (e) => {
      await finish({ exit_code: 1, stderr: [`notemd: unexpected error: ${e}`] })
    })
  })
</script>

<!-- Headless: no DOM body. -->
