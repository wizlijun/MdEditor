import { basename, dirname, abbreviateHome } from './paths'

/** One file's worth of a single device's recent history (synced via git). */
export interface SyncedEntry {
  /** Vault-relative path (set when the file lives inside the Vault). */
  rel?: string
  /** Absolute path (set when the file lives outside the Vault). */
  abs?: string
  /** Last-opened time, ms since epoch. */
  lastOpened: number
}

export interface DeviceRecents {
  deviceId: string
  deviceName: string
  entries: SyncedEntry[]
}

/** A recent resolved to an absolute path on THIS device. */
export interface ResolvedRecent {
  path: string
  lastOpened: number
}

function withTrailingSlash(root: string): string {
  return root.endsWith('/') ? root : root + '/'
}

export function isUnder(path: string, root: string): boolean {
  if (path === root) return true
  return path.startsWith(withTrailingSlash(root))
}

/** Classify a local absolute path for storage in this device's synced file. */
export function toSyncedEntry(absPath: string, lastOpened: number, vaultRoot: string | null): SyncedEntry {
  if (vaultRoot && isUnder(absPath, vaultRoot)) {
    return { rel: absPath.slice(withTrailingSlash(vaultRoot).length), lastOpened }
  }
  return { abs: absPath, lastOpened }
}

/**
 * Resolve this device's recent files (stored most-recent-first) into
 * ResolvedRecent[] whose `lastOpened` values strictly decrease in list order.
 *
 * Real timestamps are honoured where they don't violate that order; entries
 * without a stored timestamp (e.g. files opened before timestamps were tracked)
 * slot just below their predecessor. Anchoring to the previous entry — not to
 * the current wall clock — is what keeps the merged menu in
 * most-recently-opened-first order: a synthetic "now - index" fallback lets
 * legacy entries drift above genuinely-recent files as time passes.
 */
export function resolveLocalRecents(
  recentFiles: readonly string[],
  openedAt: Readonly<Record<string, number>>,
  nowAnchor: number,
): ResolvedRecent[] {
  const out: ResolvedRecent[] = []
  let ceiling = nowAnchor + 1
  for (const p of recentFiles) {
    const real = openedAt[p]
    const ts = real != null && real < ceiling ? real : ceiling - 1
    out.push({ path: p, lastOpened: ts })
    ceiling = ts
  }
  return out
}

/** Resolve a synced entry to an absolute path on this device, or null if unresolvable. */
export function resolveEntry(e: SyncedEntry, vaultRoot: string | null): ResolvedRecent | null {
  if (e.rel != null) {
    if (!vaultRoot) return null
    return { path: withTrailingSlash(vaultRoot) + e.rel, lastOpened: e.lastOpened }
  }
  if (e.abs != null) return { path: e.abs, lastOpened: e.lastOpened }
  return null
}

/**
 * Merge this device's recents with every other device's synced file.
 * Dedups by absolute path (keeping the most-recent lastOpened), drops
 * tombstoned paths, sorts newest-first, and caps at `limit`.
 */
export function mergeRecents(
  local: ResolvedRecent[],
  deviceFiles: DeviceRecents[],
  vaultRoot: string | null,
  tombstones: string[],
  limit = 10,
): string[] {
  const byPath = new Map<string, number>()
  const add = (path: string, ts: number) => {
    const prev = byPath.get(path)
    if (prev === undefined || ts > prev) byPath.set(path, ts)
  }
  for (const r of local) add(r.path, r.lastOpened)
  for (const f of deviceFiles) {
    for (const e of f.entries) {
      const r = resolveEntry(e, vaultRoot)
      if (r) add(r.path, r.lastOpened)
    }
  }
  const tomb = new Set(tombstones)
  return [...byPath.entries()]
    .filter(([p]) => !tomb.has(p))
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
    .map(([p]) => p)
}

/**
 * Native macOS menu items have no tooltip, so the full path lives in the label:
 * `filename — ~/parent/dir` (home-abbreviated).
 */
export function formatRecentLabel(absPath: string, home: string | null): string {
  const name = basename(absPath)
  const dir = dirname(absPath)
  // `dirname` returns the root itself ('/' or 'D:/') for a top-level file;
  // that is not worth showing next to the name.
  const shownDir = dir === '/' || dir === '.' || /^[a-zA-Z]:\/$/.test(dir)
    ? '' : abbreviateHome(dir, home)
  return shownDir ? `${name} — ${shownDir}` : name
}
