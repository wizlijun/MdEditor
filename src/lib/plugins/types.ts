export type Capability =
  | 'renderer.html'
  | 'renderer.raw'
  | 'settings.read'
  | `settings.write:${string}`
  | 'clipboard.write'
  | 'toast'
  | 'dialog'

export type SettingsField =
  | { key: string; type: 'string'; label: string; default?: string; placeholder?: string }
  | { key: string; type: 'secret'; label: string }
  | { key: string; type: 'select'; label: string; options: string[]; default?: string }
  | { key: string; type: 'boolean'; label: string; default?: boolean }

export interface PromptSpec {
  kind: 'save-dialog'
  default_filename: string
  filters: Array<{ name: string; extensions: string[] }>
}

export interface MenuEntry {
  location: 'file' | 'edit' | 'view' | 'window' | 'help' | 'plugins'
  /** Optional named sub-menu under `location` (e.g. 'import' → File ▸ Import).
   *  Native menu grouping only; the frontend still buckets by `location`. */
  submenu?: string
  label: string
  shortcut?: string
  command: string
  enabled_when?: string
  prompt?: PromptSpec
}

export interface ContextMenuEntry {
  location: 'tab' | 'editor'
  label: string
  command: string
  enabled_when?: string
}

/** A custom-editor contribution (子项目④): a v2 plugin claims a set of file
 *  extensions and serves an iframe editor (`entry`) for them. `id` is the
 *  editor's stable id within the plugin (multiple editors per plugin allowed). */
export interface CustomEditorContribution {
  id: string
  /** File extensions this editor handles, WITH or WITHOUT the leading dot
   *  (e.g. `'.base'` or `'base'`); the registry normalises both. */
  file_extensions: string[]
  /** UI-relative path served under `plugin://<id>/`, e.g. `'editor.html'`. */
  entry: string
}

export interface CliArg {
  name: string
  type: 'path' | 'string' | 'integer'
  required: boolean
  help?: string
}

export interface CliFlag {
  long: string                  // must start with "--"
  short?: string                // must be "-x" where x is a single ASCII letter
  type: 'boolean' | 'string'
  help?: string
}

export interface CliEntry {
  subcommand: string
  aliases?: string[]            // each must start with "-"
  command: string               // must match a command implemented by the plugin binary
  summary: string
  args?: CliArg[]
  flags?: CliFlag[]
  requires_tab_context?: boolean
}

/** Per-locale overrides for a plugin's user-facing strings (English base lives
 *  in the top-level manifest fields). Keys mirror stable ids: menu/context
 *  entries by `command`, settings fields by `key`. */
export interface PluginI18n {
  name?: string
  description?: string
  menus?: Record<string, string>
  context_menus?: Record<string, string>
  'settings.tab_label'?: string
  'settings.fields'?: Record<string, string>
}

export interface PluginManifest {
  id: string
  name: string
  version: string
  /** How the host treats the plugin. Builtins ship in-app and honor
   *  `default_enabled`; anything else (or unset) is treated as external. */
  kind?: 'builtin' | 'external' | string
  /** Boot-time default for builtin plugins when the user hasn't set an
   *  explicit `plugins.enabled.<id>` value. Ignored for external plugins. */
  default_enabled?: boolean
  description?: string
  i18n?: Record<string, PluginI18n>
  binary: string
  menus?: MenuEntry[]
  context_menus?: ContextMenuEntry[]
  /** Custom-editor contributions (子项目④), passed through by the v2 adapter.
   *  Each entry declares `{ id, file_extensions, entry }`. Absent for v1
   *  plugins and v2 plugins with no custom editors. Consumed by
   *  `buildCustomEditorRegistry` to map file extensions → editor iframes. */
  custom_editors?: CustomEditorContribution[]
  settings?: { tab_label: string; schema: SettingsField[] }
  host_capabilities: Capability[]
  timeout_seconds?: number
  /** Whole-plugin availability gate (distinct from per-menu `enabled_when`).
   *  When present and false, the plugin is not selectable in settings. */
  available_when?: string
  cli?: CliEntry[]              // new, optional
  /** `2` when this manifest comes from the v2 runtime (adapter-shaped);
   *  execution must go through `plugin_v2_execute`, not `invoke_plugin`. */
  manifest_version?: number
  /** `open_command → window_id` for v2 plugins whose window contributions
   *  declare an `open_command`. When a dispatched command is a key here, route
   *  it to `plugin_v2_open_window` instead of `plugin_v2_execute`. */
  open_windows?: Record<string, string>
}

export interface RequestContextTab {
  path: string | null
  filename: string | null
  extension: string | null
  kind: TabKind
  title: string
  is_dirty: boolean
  is_untitled: boolean
}

export interface PluginRequest {
  command: string
  context: {
    tab: RequestContextTab
    rendered_html?: string
    raw_content?: string
    output_path?: string
  }
  settings?: Record<string, unknown>
  host_version: string
  plugin_api_version: 1
}

export type ToastLevel = 'success' | 'info' | 'warn' | 'error'

export type PluginAction =
  | { type: 'toast'; level: ToastLevel; message: string; detail?: string }
  | { type: 'clipboard.write'; text: string }
  | { type: 'settings.merge'; patch: Record<string, unknown> }
  | { type: 'dialog.confirm'; title: string; message: string; if_confirm_invoke: string }
  | { type: 'dialog.message'; title: string; message: string; level: 'info' | 'warn' | 'error' }
  | { type: 'cli.result'; data: Record<string, unknown> }

export interface PluginResponse {
  success: boolean
  actions: PluginAction[]
}

export type TabKind = 'markdown' | 'html' | 'code' | 'spreadsheet' | 'base' | 'custom'

/** What we evaluate `enabled_when` expressions against. */
export interface EnabledWhenContext {
  currentTab: {
    path: string | null
    filename: string | null
    extension: string | null
    kind: TabKind | null
    hasContent: boolean
    isDirty: boolean
    isUntitled: boolean
    canSyncToVault?: boolean
    isTrackedVaultFile?: boolean
    /** True when the current file is a vault mirror AND this device has a
     *  recorded source path (drives the "Reveal Sync Source" menu item). */
    hasSyncSource?: boolean
    /** True when the current file lives inside the configured vault git repo
     *  (drives the git-history menu item's enabled state). */
    isInVault?: boolean
  } | null
  settings: Record<string, unknown>
  /** True once the user has configured a Vault (sotvault root is set). */
  vaultConfigured: boolean
}
