// Bridge-backed MediaResolver for the Editor Kit.
//
// The kit runs inside an *isolated* plugin webview: no Tauri IPC, no
// `@tauri-apps/plugin-fs`. Local files are read through the plugin host bridge
// (`window.notemd.request('host.vault.read_bytes')`), which returns base64 and
// is gated by the `vault.read` capability + path containment on the Rust side.
//
// Structure mirrors `src/lib/adapters/tauri-media-resolver.ts` (blob cache +
// extension→MIME tables + empty string on failure); only the byte source
// differs. Keep the two in sync when either changes.

import type { MediaResolver } from '@moraya/core'

const blobCache = new Map<string, string>()

const IMAGE_MIME: Record<string, string> = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
  gif: 'image/gif', svg: 'image/svg+xml', webp: 'image/webp',
  ico: 'image/x-icon', bmp: 'image/bmp', avif: 'image/avif',
}

const MEDIA_MIME: Record<string, string> = {
  mp4: 'video/mp4', webm: 'video/webm', ogg: 'video/ogg', ogv: 'video/ogg',
  mov: 'video/quicktime', avi: 'video/x-msvideo',
  mp3: 'audio/mpeg', wav: 'audio/wav', flac: 'audio/flac', aac: 'audio/aac',
  m4a: 'audio/mp4', oga: 'audio/ogg', opus: 'audio/opus', weba: 'audio/webm',
}

interface Bridge {
  request(method: string, params?: unknown): Promise<Record<string, unknown>>
}

function bridge(): Bridge | null {
  const b = (window as unknown as { notemd?: Bridge }).notemd
  return b && typeof b.request === 'function' ? b : null
}

function pathExt(path: string): string {
  const basename = path.split('/').pop() ?? ''
  const dot = basename.lastIndexOf('.')
  return dot > 0 ? basename.slice(dot + 1).toLowerCase() : ''
}

function buildBlob(bytes: Uint8Array, mime: string): string {
  const blob = new Blob([bytes.buffer as ArrayBuffer], { type: mime })
  return URL.createObjectURL(blob)
}

function decodeBase64(base64: string): Uint8Array {
  const bin = atob(base64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return bytes
}

/**
 * Absolute path → vault-relative path, or `null` when the file lives outside
 * the vault (the bridge only serves vault-internal paths).
 *
 * moraya hands the resolver *absolute* paths: relative image sources in the
 * document are joined against the base dir set via `setDocumentBaseDir()`.
 */
export function toVaultRelative(vaultRoot: string, absolutePath: string): string | null {
  if (!vaultRoot) return null
  const root = vaultRoot.endsWith('/') ? vaultRoot.slice(0, -1) : vaultRoot
  if (!absolutePath.startsWith(root + '/')) return null
  const rel = absolutePath.slice(root.length + 1)
  return rel.length > 0 ? rel : null
}

/** Ask the host for the vault root; empty string when unset or unavailable. */
export async function loadVaultRoot(): Promise<string> {
  const b = bridge()
  if (!b) return ''
  try {
    const info = await b.request('host.vault.info', {})
    const root = info?.root
    return typeof root === 'string' ? root : ''
  } catch {
    return ''
  }
}

/**
 * A `MediaResolver` (see `@moraya/core` `src/types.ts`) that serves vault files
 * through the plugin bridge. Anything the bridge cannot serve resolves to an
 * empty string — the same failure behaviour as the desktop Tauri resolver, so
 * the `<img>` simply renders broken instead of throwing inside a NodeView.
 */
export function bridgeMediaResolver(vaultRoot: string): MediaResolver {
  async function load(absolutePath: string, mimes: Record<string, string>, fallbackMime: string): Promise<string> {
    const cached = blobCache.get(absolutePath)
    if (cached) return cached
    const rel = toVaultRelative(vaultRoot, absolutePath)
    if (!rel) return ''
    const b = bridge()
    if (!b) return ''
    try {
      const res = await b.request('host.vault.read_bytes', { path: rel })
      const base64 = res?.base64
      if (typeof base64 !== 'string') return ''
      const url = buildBlob(decodeBase64(base64), mimes[pathExt(absolutePath)] || fallbackMime)
      blobCache.set(absolutePath, url)
      return url
    } catch {
      return ''
    }
  }

  return {
    loadLocalImage: (absolutePath) => load(absolutePath, IMAGE_MIME, 'image/png'),
    loadLocalMedia: (absolutePath) => load(absolutePath, MEDIA_MIME, 'application/octet-stream'),
    // No plugin-http in a plugin webview either; hand the URL back and let the
    // WebView fetch it (subject to the window's CSP).
    loadRemoteMedia: async (url) => url,
  }
}
