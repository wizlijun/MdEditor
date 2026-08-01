import type { PluginManifest, Capability } from './types'

const VALID_CAPS = new Set<string>([
  'renderer.html', 'renderer.raw', 'settings.read',
  'clipboard.write', 'toast', 'dialog',
])

export const BUILTIN_SUBCOMMANDS = ['help', 'version', 'plugin']
const RESERVED_GLOBAL_FLAGS = [
  '-h', '--help', '-v', '--version',
  '-q', '--quiet', '--json',
  '--no-clipboard', '--yes', '-y',
]
const SUBCOMMAND_RE = /^[a-z][a-z0-9-]{1,31}$/
const SHORT_FLAG_RE = /^-[a-zA-Z]$/
const LONG_FLAG_RE = /^--[a-z][a-z0-9-]*$/

function isValidCapability(c: string): c is Capability {
  if (VALID_CAPS.has(c)) return true
  if (c.startsWith('settings.write:') && c.length > 'settings.write:'.length) return true
  return false
}

const ID_RE = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/

export type ValidateResult =
  | { ok: true; value: PluginManifest }
  | { ok: false; error: string }

export function validateManifest(m: unknown): ValidateResult {
  if (m == null || typeof m !== 'object') return { ok: false, error: 'manifest must be an object' }
  const o = m as Record<string, unknown>

  if (typeof o.id !== 'string' || !ID_RE.test(o.id))
    return { ok: false, error: 'id must be lowercase kebab-case' }
  if (typeof o.name !== 'string' || o.name.length === 0)
    return { ok: false, error: 'name required' }
  if (typeof o.version !== 'string' || o.version.length === 0)
    return { ok: false, error: 'version required' }
  if (typeof o.binary !== 'string' || o.binary.length === 0)
    return { ok: false, error: 'binary required' }

  if (!Array.isArray(o.host_capabilities))
    return { ok: false, error: 'host_capabilities must be an array' }
  for (const c of o.host_capabilities) {
    if (typeof c !== 'string' || !isValidCapability(c))
      return { ok: false, error: `unknown capability: ${String(c)}` }
  }

  if (o.settings != null) {
    const s = o.settings as Record<string, unknown>
    if (typeof s.tab_label !== 'string')
      return { ok: false, error: 'settings.tab_label must be string' }
    if (!Array.isArray(s.schema))
      return { ok: false, error: 'settings.schema must be an array' }
    for (const f of s.schema) {
      const fr = f as Record<string, unknown>
      if (typeof fr.key !== 'string' || !fr.key.startsWith(`${o.id}.`))
        return { ok: false, error: `settings field key '${String(fr.key)}' must start with '${o.id}.'` }
    }
  }

  if (Array.isArray(o.menus)) {
    for (const me of o.menus) {
      const mr = me as Record<string, unknown>
      if (mr.prompt != null) {
        const p = mr.prompt as Record<string, unknown>
        if (p.kind !== 'save-dialog')
          return { ok: false, error: `unsupported prompt.kind: ${String(p.kind)}` }
        if (typeof p.default_filename !== 'string' || p.default_filename.length === 0)
          return { ok: false, error: 'prompt.default_filename required' }
        if (!Array.isArray(p.filters))
          return { ok: false, error: 'prompt.filters must be an array' }
      }
    }
  }

  if (o.cli != null) {
    if (!Array.isArray(o.cli))
      return { ok: false, error: 'cli must be an array' }
    for (const ce of o.cli) {
      const cr = ce as Record<string, unknown>
      if (typeof cr.subcommand !== 'string' || !SUBCOMMAND_RE.test(cr.subcommand))
        return { ok: false, error: `cli.subcommand invalid: ${String(cr.subcommand)}` }
      if (BUILTIN_SUBCOMMANDS.includes(cr.subcommand))
        return { ok: false, error: `cli.subcommand '${cr.subcommand}' collides with builtin` }
      if (typeof cr.command !== 'string' || cr.command.length === 0)
        return { ok: false, error: 'cli.command required' }
      if (typeof cr.summary !== 'string' || cr.summary.length === 0)
        return { ok: false, error: 'cli.summary required' }
      if (cr.aliases != null) {
        if (!Array.isArray(cr.aliases))
          return { ok: false, error: 'cli.aliases must be an array' }
        for (const a of cr.aliases) {
          if (typeof a !== 'string' || !a.startsWith('-'))
            return { ok: false, error: `cli.alias must start with '-': ${String(a)}` }
          if (RESERVED_GLOBAL_FLAGS.includes(a))
            return { ok: false, error: `cli.alias '${a}' collides with reserved global flag` }
          if (!SHORT_FLAG_RE.test(a) && !LONG_FLAG_RE.test(a))
            return { ok: false, error: `cli.alias has invalid format: ${a}` }
        }
      }
      if (cr.flags != null) {
        if (!Array.isArray(cr.flags))
          return { ok: false, error: 'cli.flags must be an array' }
        for (const f of cr.flags) {
          const fr = f as Record<string, unknown>
          if (typeof fr.long !== 'string' || !LONG_FLAG_RE.test(fr.long))
            return { ok: false, error: `cli.flag.long invalid: ${String(fr.long)}` }
          if (RESERVED_GLOBAL_FLAGS.includes(fr.long))
            return { ok: false, error: `cli.flag.long '${fr.long}' collides with reserved global flag` }
          if (fr.short != null && (typeof fr.short !== 'string' || !SHORT_FLAG_RE.test(fr.short)))
            return { ok: false, error: `cli.flag.short invalid: ${String(fr.short)}` }
          if (fr.short != null && RESERVED_GLOBAL_FLAGS.includes(fr.short as string))
            return { ok: false, error: `cli flag short '${String(fr.short)}' is a reserved global flag` }
        }
      }
      if (cr.args != null && !Array.isArray(cr.args))
        return { ok: false, error: 'cli.args must be an array' }
    }
  }

  return { ok: true, value: o as unknown as PluginManifest }
}

export interface Registry {
  byId: Record<string, PluginManifest>
  errors: string[]
}

export function buildRegistry(manifests: PluginManifest[]): Registry {
  const byId: Record<string, PluginManifest> = {}
  const errors: string[] = []
  for (const m of manifests) {
    if (m.id in byId) { errors.push(`duplicate plugin id '${m.id}' — keeping first`); continue }
    byId[m.id] = m
  }

  // Detect and drop conflicting cli entries; non-cli fields are unaffected.
  const accepted = Object.values(byId)
  const conflicts = findCliConflicts(accepted, BUILTIN_SUBCOMMANDS)
  // Per-plugin set of (kind:key) pairs that should be removed from the plugin's cli.
  const dropByPlugin = new Map<string, Array<{ kind: 'subcommand' | 'alias'; key: string }>>()
  for (const c of conflicts) {
    if (c.kind === 'subcommand') {
      if (c.reservedCore) {
        const owners = c.owners.map(o => o.pluginId).join(', ')
        errors.push(`cli subcommand '${c.key}' is a reserved built-in — dropped from '${owners}'`)
      } else {
        const owners = c.owners.map(o => o.pluginId).join(', ')
        errors.push(`cli subcommand '${c.key}' claimed by multiple plugins: ${owners} — dropped from all`)
      }
    } else {
      const owners = c.owners.map(o => o.pluginId).join(', ')
      errors.push(`cli alias '${c.key}' claimed by multiple plugins: ${owners} — dropped from all`)
    }
    for (const o of c.owners) {
      const arr = dropByPlugin.get(o.pluginId) ?? []
      arr.push({ kind: c.kind, key: c.key })
      dropByPlugin.set(o.pluginId, arr)
    }
  }
  for (const [pluginId, drops] of dropByPlugin) {
    const orig = byId[pluginId]
    if (!orig) continue
    const filteredCli = (orig.cli ?? []).filter(entry => {
      for (const d of drops) {
        if (d.kind === 'subcommand' && entry.subcommand === d.key) return false
        if (d.kind === 'alias' && (entry.aliases ?? []).includes(d.key)) return false
      }
      return true
    })
    // Return a NEW manifest object; do not mutate the input.
    byId[pluginId] = { ...orig, cli: filteredCli }
  }

  return { byId, errors }
}

export interface CliConflict {
  kind: 'subcommand' | 'alias'
  key: string
  owners: { pluginId: string; subcommand: string }[]
  reservedCore?: boolean
}

export function findCliConflicts(
  manifests: PluginManifest[],
  builtinSubcommands: string[],
): CliConflict[] {
  const subMap = new Map<string, CliConflict>()
  const aliasMap = new Map<string, CliConflict>()
  for (const m of manifests) {
    for (const entry of m.cli ?? []) {
      const sub = subMap.get(entry.subcommand) ?? {
        kind: 'subcommand', key: entry.subcommand, owners: [],
      }
      sub.owners.push({ pluginId: m.id, subcommand: entry.subcommand })
      subMap.set(entry.subcommand, sub)
      for (const a of entry.aliases ?? []) {
        const al = aliasMap.get(a) ?? { kind: 'alias', key: a, owners: [] }
        al.owners.push({ pluginId: m.id, subcommand: entry.subcommand })
        aliasMap.set(a, al)
      }
    }
  }
  const conflicts: CliConflict[] = []
  for (const [key, c] of subMap) {
    const reserved = builtinSubcommands.includes(key)
    if (c.owners.length > 1 || reserved) {
      if (reserved) c.reservedCore = true
      conflicts.push(c)
    }
  }
  for (const [, c] of aliasMap) {
    if (c.owners.length > 1) conflicts.push(c)
  }
  return conflicts
}

// --- Active plugin IDs (filled at startup from Rust's get_plugin_manifests) ---

let _activePluginIds = new Set<string>()

export function isPluginActive(id: string): boolean {
  return _activePluginIds.has(id)
}

/** Test-only: replace the active set without hitting IPC. */
export function setActivePluginIds(ids: Set<string>): void {
  _activePluginIds = ids
}

export async function initActivePluginIds(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    const list = await invoke<Array<{ id: string }>>('get_plugin_manifests')
    _activePluginIds = new Set(list.map(m => m.id))
  } catch (e) {
    console.warn('[plugins/registry] initActivePluginIds:', e)
    _activePluginIds = new Set()
  }
}
