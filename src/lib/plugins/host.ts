import type { PluginManifest, PluginRequest, TabKind } from './types'

/**
 * Builds the request context a plugin command receives. Plugins run on the v2
 * runtime (`plugin_v2_execute` / `plugin_v2_open_window`); this module only
 * assembles the payload — the invocation itself lives at the call sites
 * (App.svelte for the GUI, CliRunner.svelte for the CLI).
 */

export interface TabSnapshot {
  path: string | null
  filename: string | null
  extension: string | null
  kind: TabKind
  title: string
  isDirty: boolean
  isUntitled: boolean
  content: string
}

export interface BuildContextOpts {
  htmlBaker?: (tab: TabSnapshot) => Promise<string>
  settingsReader?: (pluginId: string) => Record<string, unknown>
  /**
   * If the menu item that triggered this invoke declared a `prompt` block
   * (e.g. save-dialog), the dispatcher should resolve the user's chosen
   * path and pass it here. The host serialises it into context.output_path
   * so the plugin can use it without the host needing per-plugin code.
   */
  outputPath?: string
  /**
   * CLI invocation only: all manifest-declared positional arguments and the
   * parsed `--flag` values, forwarded verbatim into `context.cli`. This is
   * the ONLY thing standing between a plugin's `cli_str`/`cli_flag` helper
   * (which probes `/cli/args/<name>`, `/cli/flags/<name>`, `/cli/<name>`,
   * `/<name>` — see e.g. `ebook-import`'s or `roam-import`'s `plugin.rs`)
   * and every `--flag` a CLI user typed: without this, those probes always
   * miss and the plugin silently falls back to its own defaults no matter
   * what was passed on the command line.
   */
  cli?: { args?: Record<string, string | number>; flags?: Record<string, string | boolean> }
}

export async function buildContext(
  manifest: PluginManifest,
  tab: TabSnapshot,
  opts: BuildContextOpts,
): Promise<{ context: PluginRequest['context']; settings: PluginRequest['settings'] }> {
  const ctx: PluginRequest['context'] = {
    tab: {
      path: tab.path,
      filename: tab.filename,
      extension: tab.extension,
      kind: tab.kind,
      title: tab.title,
      is_dirty: tab.isDirty,
      is_untitled: tab.isUntitled,
    },
  }
  if (manifest.host_capabilities.includes('renderer.raw')) {
    ctx.raw_content = tab.content
  }
  if (manifest.host_capabilities.includes('renderer.html')) {
    if (!opts.htmlBaker) throw new Error('plugin needs renderer.html but no htmlBaker provided')
    ctx.rendered_html = await opts.htmlBaker(tab)
  }
  if (opts.outputPath != null) {
    ctx.output_path = opts.outputPath
  }
  if (opts.cli != null) {
    ctx.cli = opts.cli
  }
  let settings: PluginRequest['settings'] | undefined
  if (manifest.host_capabilities.includes('settings.read') && opts.settingsReader) {
    settings = opts.settingsReader(manifest.id)
  }
  return { context: ctx, settings }
}
