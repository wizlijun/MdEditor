import { invoke } from '@tauri-apps/api/core'
import type { MediaResolver } from '@moraya/core'

export const CANVAS_BLOB_CACHE_BUDGET = 32 * 1024 * 1024

export interface CanvasResourceImportResult {
  relativePath: string
  canonicalPath: string
  size: number
}

export interface CanvasResourceResolveResult {
  canonicalPath: string
}

interface CacheEntry { url: string; size: number }

const MIME_BY_ID: Readonly<Record<number, string>> = {
  1: 'image/png',
  2: 'image/jpeg',
  3: 'image/gif',
  4: 'image/webp',
  5: 'image/bmp',
  6: 'image/x-icon',
  7: 'image/avif',
  8: 'image/heic',
  9: 'image/heif',
}

/**
 * Canvas-scoped MediaResolver. Every byte comes from the Rust containment
 * boundary; remote media and local audio/video are denied. Blob URLs belong
 * to one Canvas surface and are revoked together when that surface closes.
 */
export class CanvasResourceSession implements MediaResolver {
  readonly root: string
  private readonly budget: number
  private cache = new Map<string, CacheEntry>()
  private pending = new Map<string, Promise<string>>()
  private urls = new Set<string>()
  private cachedBytes = 0
  private disposed = false

  constructor(root: string, budget = CANVAS_BLOB_CACHE_BUDGET) {
    this.root = root
    this.budget = budget
  }

  peek(target: string): string | null {
    const cached = this.cache.get(target)
    if (!cached) return null
    this.cache.delete(target)
    this.cache.set(target, cached)
    return cached.url
  }

  async loadLocalImage(target: string): Promise<string> {
    if (this.disposed || !target) return ''
    const cached = this.cache.get(target)
    if (cached) {
      this.cache.delete(target)
      this.cache.set(target, cached)
      return cached.url
    }
    const current = this.pending.get(target)
    if (current) return current
    if (this.pending.size >= 128) return ''
    const request = this.readImage(target)
    this.pending.set(target, request)
    try {
      return await request
    } finally {
      if (this.pending.get(target) === request) this.pending.delete(target)
    }
  }

  async loadLocalMedia(_target: string): Promise<string> {
    return ''
  }

  async loadRemoteMedia(_url: string): Promise<string> {
    return ''
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    for (const url of this.urls) URL.revokeObjectURL(url)
    this.urls.clear()
    this.cache.clear()
    this.pending.clear()
    this.cachedBytes = 0
  }

  private async readImage(target: string): Promise<string> {
    try {
      const response = await invoke<ArrayBuffer | Uint8Array>('canvas_resource_read', {
        root: this.root,
        target,
      })
      if (this.disposed) return ''
      const payload = response instanceof Uint8Array ? response : new Uint8Array(response)
      const mime = MIME_BY_ID[payload[0]]
      const bytes = payload.subarray(1)
      if (!mime || bytes.byteLength === 0 || bytes.byteLength > this.budget) return ''
      while (this.cachedBytes + bytes.byteLength > this.budget) {
        const oldest = this.cache.entries().next().value as [string, CacheEntry] | undefined
        if (!oldest) return ''
        const [oldestTarget, entry] = oldest
        this.cache.delete(oldestTarget)
        this.urls.delete(entry.url)
        this.cachedBytes -= entry.size
        URL.revokeObjectURL(entry.url)
      }
      const body = Uint8Array.from(bytes)
      const blob = new Blob([body], { type: mime })
      const url = URL.createObjectURL(blob)
      if (this.disposed) {
        URL.revokeObjectURL(url)
        return ''
      }
      this.cache.set(target, { url, size: bytes.byteLength })
      this.urls.add(url)
      this.cachedBytes += bytes.byteLength
      return url
    } catch {
      return ''
    }
  }
}

export async function importCanvasResource(
  root: string,
  canvasPath: string,
  sourcePath: string,
): Promise<CanvasResourceImportResult> {
  return invoke<CanvasResourceImportResult>('canvas_resource_import', {
    root,
    canvasPath,
    sourcePath,
  })
}

export async function resolveCanvasResource(root: string, target: string): Promise<string> {
  const result = await invoke<CanvasResourceResolveResult>('canvas_resource_resolve', { root, target })
  return result.canonicalPath
}
