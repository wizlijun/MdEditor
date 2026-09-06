<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { buildContext } from '../plugins/host'
  import { renderTabAsInlineBody } from '../plugins/host-render-html'
  import { mkdir, writeTextFile } from '@tauri-apps/plugin-fs'
  import { getPluginScopedAll, loadSettings } from '../settings.svelte'
  import { generateInsightsReport } from '../insights/run'
  import { runShareCli, buildVirtualTab } from './share-cli'
  import { firstPathArg, outputPathFor, requiresFileArg, type CliPayload } from './cli-runner'
  import { runReadingInsightsCli } from './reading-insights-cli'
  import { finishForPluginCliResult } from './plugin-result'
  import type { PluginManifest, TabKind } from '../plugins/types'
  import type { FileKind } from '../fs'

  let activePayload: CliPayload | null = null

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
    await runReadingInsightsCli(payload, {
      finish,
      resolveVault: () => invoke<string | null>('sotvault_vault_root'),
      generate: generateInsightsReport,
      mkdir: async (path) => { await mkdir(path, { recursive: true }) },
      writeTextFile,
    })
  }

  async function run(): Promise<void> {
    let payload: CliPayload
    try {
      payload = await invoke<CliPayload>('cli_payload')
      activePayload = payload
    } catch (e) {
      await finish({ exit_code: 1, stderr: [`notemd: failed to fetch cli payload: ${e}`] })
      return
    }

    // Hydrate the in-memory settings store from disk BEFORE anything writes
    // to it. Without this, the runner sees defaults for every key, and the
    // first save (e.g., updating share.records) wipes the user's stored
    // apiKey / baseUrl / recentFiles.
    try {
      await loadSettings()
    } catch (e) {
      const message = `failed to load settings: ${e}`
      await finish({
        exit_code: 1,
        stdout: payload.global.json ? JSON.stringify({ ok: false, error: { code: 'settings_error', message } }) : undefined,
        stderr: payload.global.json ? [] : [`notemd: ${message}`],
      })
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
      const message = isV2
          ? `v2 plugin '${payload.plugin_id}' is not installed or the v2 runtime flag is off.`
          : `plugin '${payload.plugin_id}' is not enabled. Run 'notemd plugin enable ${payload.plugin_id}'.`
      await finish({
        exit_code: 3,
        stdout: payload.global.json ? JSON.stringify({ ok: false, error: { code: 'plugin_unavailable', message } }) : undefined,
        stderr: payload.global.json ? [] : [`notemd: ${message}`],
      })
      return
    }
    const entry = (manifest.cli ?? []).find(c => c.subcommand === payload.subcommand)
    const inputPath = firstPathArg(entry, payload.args)
    if (requiresFileArg(entry) && !inputPath) {
      const message = 'missing file argument'
      await finish({
        exit_code: 2,
        stdout: payload.global.json ? JSON.stringify({ ok: false, error: { code: 'invalid_arguments', message } }) : undefined,
        stderr: payload.global.json ? [] : [`notemd: ${message}`],
      })
      return
    }

    // A path can be a directory or binary source owned by the plugin. Entries
    // with tab context disabled forward the path without loading a document.
    const needsTab = inputPath && entry?.requires_tab_context !== false
    const built = needsTab ? await buildVirtualTab(inputPath, finish, payload.global.json) : null
    if (needsTab && !built) return

    // For commands requiring rendered HTML, bake the content. Never runs
    // without a tab — a file-less command cannot request tab context.
    let renderedHtml: string | undefined
    if (built && entry?.requires_tab_context && manifest.host_capabilities.includes('renderer.html')) {
      try {
        renderedHtml = built.fileKind === 'image' ? '' : await renderTabAsInlineBody(built.tab)
      } catch (e) {
        const message = `render failed: ${e}`
        await finish({
          exit_code: 1,
          stdout: payload.global.json ? JSON.stringify({ ok: false, error: { code: 'render_failed', message } }) : undefined,
          stderr: payload.global.json ? [] : [`notemd: ${message}`],
        })
        return
      }
    }

    // Resolve output_path for plugins that need it (e.g. md2pdf export).
    // Meaningless without a file to derive/anchor it to, so skip entirely.
    let outputPath: string | undefined
    if (inputPath) {
      const outputFlag = payload.flags['output'] as string | undefined
      if (outputFlag) {
        outputPath = outputPathFor(inputPath, outputFlag)
      } else if (manifest.id === 'md2pdf' || manifest.id === 'notemd.md2pdf') {
        outputPath = outputPathFor(inputPath)
      }
    }

    const pluginSettings = getPluginScopedAll(manifest.id)

    // Commands without a tab pass an empty snapshot; their path arguments
    // still reach the backend through context.cli.args below.
    const snap = built
      ? {
          path: built.tab.filePath,
          filename: built.tab.title,
          extension: built.extension,
          kind: toTabKind(built.tab.kind),
          title: built.tab.title,
          isDirty: false,
          isUntitled: false,
          content: built.tab.currentContent,
        }
      : {
          path: '',
          filename: null,
          extension: null,
          kind: 'markdown' as TabKind,
          title: '',
          isDirty: false,
          isUntitled: true,
          content: '',
        }
    const invokeOpts = {
      htmlBaker: renderedHtml != null ? async () => renderedHtml! : undefined,
      settingsReader: () => pluginSettings,
      outputPath,
      // Every plugin's `cli_str`/`cli_flag` helper probes context.cli.args /
      // context.cli.flags first — without forwarding the parsed payload here,
      // those probes always miss and every `--flag` the user typed is
      // silently ignored (see host.ts's `BuildContextOpts.cli` doc comment).
      cli: { args: payload.args, flags: payload.flags },
    }

    // The command executes on the plugin's resident runtime via
    // plugin_v2_execute_cli, which returns a result value (toasts are GUI-only
    // events). Output conventions: --json wraps the result as {ok,data}; an
    // opt-in structured result envelope lets a plugin return its full report
    // while requesting a non-zero exit code. Thrown errors still exit 4 with
    // a plugin_failed envelope; --quiet suppresses only successful output.
    try {
      const { context } = await buildContext(manifest, snap, invokeOpts)
      const rawData = await invoke<unknown>('plugin_v2_execute_cli', {
        pluginId: manifest.id,
        subcommand: payload.subcommand,
        command: payload.plugin_command,
        context,
      })
      await finish(finishForPluginCliResult(rawData, {
        json: payload.global.json,
        quiet: payload.global.quiet,
        pluginName: manifest.name,
      }))
    } catch (e) {
      const message = String(e)
      await finish({
        exit_code: 4,
        stdout: payload.global.json
          ? JSON.stringify({ ok: false, error: { code: 'plugin_failed', message } })
          : undefined,
        stderr: payload.global.json ? [] : [`✗ ${manifest.name}: ${message}`],
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
      const message = `unexpected error: ${e}`
      await finish({
        exit_code: 1,
        stdout: activePayload?.global.json
          ? JSON.stringify({ ok: false, error: { code: 'internal_error', message } })
          : undefined,
        stderr: activePayload?.global.json ? [] : [`notemd: ${message}`],
      })
    })
  })
</script>

<!-- Headless: no DOM body. -->
