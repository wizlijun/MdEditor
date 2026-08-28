// Shapes the plugin market window consumes from the Rust market commands.
// These mirror `RegistryEntry` (market.rs) and `plugin_market_installed`.

import { t } from '../i18n/store.svelte'
import type { Messages } from '../i18n/en'

/** One entry of the registry index (`plugin_market_index` → RegistryIndex). */
export interface RegistryEntry {
  id: string
  version: string
  min_host: string
  archs: string[]
  size: number
  sha256: Record<string, string>
  name: string
  category?: string | null
  description: string | null
  i18n?: unknown
  icon_url?: string | null
  changelog_url?: string | null
  download: Record<string, string>
}

export interface RegistryIndex {
  plugins: RegistryEntry[]
}

/** One installed v2 plugin (`plugin_market_installed`). */
export interface InstalledV2 {
  id: string
  version: string
  enabled: boolean
  name: string | null
  category?: string | null
  capabilities: string[]
}

/** Unified installed-list row: a v1 builtin/external plugin or a v2 plugin. */
export interface InstalledRow {
  kind: 'v1' | 'v2'
  id: string
  name: string
  category?: string | null
  version: string
  enabled: boolean
  capabilities: string[]
  /** Newer version available in the registry (v2 only). */
  updateTo?: string | null
}

/**
 * True when `candidate` is a strictly newer semver than `current`. Compares
 * the dotted numeric components left-to-right; any non-numeric/pre-release
 * suffix on a component is ignored (best-effort — the registry ships clean
 * `x.y.z` versions). Missing components read as 0.
 */
export function isNewerVersion(candidate: string, current: string): boolean {
  const a = parseVersion(candidate)
  const b = parseVersion(current)
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0
    const y = b[i] ?? 0
    if (x !== y) return x > y
  }
  return false
}

function parseVersion(v: string): number[] {
  return v.split('.').map((p) => parseInt(p, 10) || 0)
}

/**
 * Human-readable label for a capability string. `settings.write:*` and any
 * other `settings.*` scope collapse to the `settings` label; unknown
 * capabilities render verbatim so a new capability never disappears silently.
 */
export function capabilityLabel(cap: string): string {
  const key = capabilityKey(cap)
  if (key) return t(key)
  return cap
}

/** Whether a capability warrants a warning highlight (sensitive host access). */
export function isSensitiveCapability(cap: string): boolean {
  const norm = normalizeCapability(cap)
  return norm === 'vault.write' || norm === 'secrets' || norm === 'location'
}

/** Map a raw capability to its normalized bucket (settings.* → settings). */
function normalizeCapability(cap: string): string {
  if (cap === 'settings' || cap.startsWith('settings.write') || cap.startsWith('settings.')) {
    return 'settings'
  }
  return cap
}

/** Map a capability to its i18n key, or null when unknown (render verbatim). */
function capabilityKey(cap: string): keyof Messages | null {
  const norm = normalizeCapability(cap)
  const table: Record<string, keyof Messages> = {
    'renderer.html': 'capability.renderer.html',
    settings: 'capability.settings',
    secrets: 'capability.secrets',
    storage: 'capability.storage',
    'vault.read': 'capability.vault.read',
    'vault.write': 'capability.vault.write',
    dialog: 'capability.dialog',
    'clipboard.write': 'capability.clipboard.write',
    location: 'capability.location',
    toast: 'capability.toast',
    'editor.events': 'capability.editor.events',
    'fs.read:dialog': 'capability.fs.read.dialog',
  }
  return table[norm] ?? null
}
