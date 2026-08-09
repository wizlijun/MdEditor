/**
 * Separator-agnostic path helpers — the single place the frontend is allowed to
 * take a filesystem path apart.
 *
 * Why this exists: Rust hands back native paths, so on Windows every path the
 * frontend receives is `D:\vault\note.md`. The codebase was written against
 * POSIX and split on `'/'` in ~20 places, which on Windows silently yields the
 * whole path as its own basename — file-tree labels, tab titles, breadcrumbs
 * and vault-relative computations all degrade at once.
 *
 * The internal canonical form is forward slashes (`normalize`). Windows APIs
 * accept them, so normalized paths can be handed straight back to `invoke()`
 * without converting back.
 *
 * Deliberately dependency-free: `base/scan.ts` and `editor-kit/` both import
 * it, and neither may pull in Tauri or Svelte runes.
 *
 * NOTE: these are for *filesystem* paths. Markdown link targets are POSIX by
 * specification — do not route those through here.
 */

/** Matches a Windows drive prefix (`C:`), with or without a following slash. */
const DRIVE_RE = /^[a-zA-Z]:/

/** Backslashes → forward slashes, with redundant separators collapsed.
 *  A leading `//` is preserved so UNC paths (`//server/share`) survive. */
export function normalize(p: string): string {
  if (!p) return p
  const slashed = p.replace(/\\/g, '/')
  const unc = slashed.startsWith('//')
  const collapsed = slashed.replace(/\/{2,}/g, '/')
  return unc ? '/' + collapsed : collapsed
}

/**
 * The non-splittable prefix of a path: `'/'`, `'C:/'`, `'//server/share/'`,
 * or `''` for a relative path. `dirname` must never chew past this.
 */
export function pathRoot(p: string): string {
  const s = normalize(p)
  if (s.startsWith('//')) {
    // //server/share — the share itself is the root, not the server.
    const m = /^\/\/[^/]+\/[^/]+\//.exec(s + '/')
    return m ? m[0] : s
  }
  if (DRIVE_RE.test(s)) return s.slice(0, 2) + '/'
  if (s.startsWith('/')) return '/'
  return ''
}

/** True for `/x`, `\x`, `C:/x`, `C:\x` and `//server/share`. */
export function isAbsolute(p: string): boolean {
  return /^([a-zA-Z]:[\\/]|[\\/])/.test(p)
}

/** Final path segment. Trailing separators are ignored. */
export function basename(p: string): string {
  if (!p) return p
  const s = normalize(p).replace(/\/+$/, '')
  const i = s.lastIndexOf('/')
  const name = i >= 0 ? s.slice(i + 1) : s
  return name || p
}

/**
 * Parent directory. Returns the path's own root (`'/'`, `'C:/'`) when the
 * input already sits at the top, so callers never walk off the drive.
 */
export function dirname(p: string): string {
  if (!p) return p
  const s = normalize(p)
  const root = pathRoot(s)
  const body = s.slice(root.length).replace(/\/+$/, '')
  const i = body.lastIndexOf('/')
  if (i < 0) return root || '.'
  return root + body.slice(0, i)
}

/** Join a directory and a child name in the canonical (forward-slash) form. */
export function joinPath(dir: string, name: string): string {
  const d = normalize(dir).replace(/\/+$/, '')
  const n = normalize(name).replace(/^\/+/, '')
  if (!d) return n
  // `D:` trimmed to nothing above would produce `D:name`; keep the separator.
  return d.endsWith(':') ? `${d}/${n}` : `${d}/${n}`
}

/**
 * `p` expressed relative to `root`, or `null` when `p` is not inside `root`.
 * Comparison is case-sensitive: Windows filesystems are case-insensitive, but
 * both sides come from the same source here, so lowering would be a guess that
 * could collide on a case-sensitive vault synced from macOS.
 */
export function relative(root: string, p: string): string | null {
  const r = normalize(root).replace(/\/+$/, '')
  const s = normalize(p)
  if (s === r) return ''
  const prefix = r.endsWith(':') ? r + '/' : r + '/'
  return s.startsWith(prefix) ? s.slice(prefix.length) : null
}

/** True when `file` sits strictly inside `dir`, at any depth. */
export function isWithinDir(file: string, dir: string): boolean {
  return relative(dir, file) !== null && normalize(file) !== normalize(dir).replace(/\/+$/, '')
}

/** `~`-abbreviate a path under `home`, for display only. */
export function abbreviateHome(p: string, home: string | null): string {
  if (!home) return p
  const h = normalize(home).replace(/\/+$/, '')
  const s = normalize(p)
  if (!h) return s
  if (s === h) return '~'
  return s.startsWith(h + '/') ? '~' + s.slice(h.length) : s
}
