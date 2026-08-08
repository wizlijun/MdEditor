import { readFile } from '@tauri-apps/plugin-fs'
import type { MediaResolver } from '@moraya/core'
import { basename } from '../paths'

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

function pathExt(path: string): string {
  const base = basename(path)
  const dot = base.lastIndexOf('.')
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : ''
}

function buildBlob(bytes: Uint8Array, mime: string): string {
  const blob = new Blob([bytes.buffer as ArrayBuffer], { type: mime })
  return URL.createObjectURL(blob)
}

export class TauriMediaResolver implements MediaResolver {
  async loadLocalImage(absolutePath: string): Promise<string> {
    const cached = blobCache.get(absolutePath)
    if (cached) return cached
    try {
      const bytes = await readFile(absolutePath)
      const mime = IMAGE_MIME[pathExt(absolutePath)] || 'image/png'
      const url = buildBlob(bytes, mime)
      blobCache.set(absolutePath, url)
      return url
    } catch {
      return ''
    }
  }

  async loadLocalMedia(absolutePath: string): Promise<string> {
    const cached = blobCache.get(absolutePath)
    if (cached) return cached
    try {
      const bytes = await readFile(absolutePath)
      const mime = MEDIA_MIME[pathExt(absolutePath)] || 'application/octet-stream'
      const url = buildBlob(bytes, mime)
      blobCache.set(absolutePath, url)
      return url
    } catch {
      return ''
    }
  }

  async loadRemoteMedia(url: string): Promise<string> {
    // mdeditor has no plugin-http; return URL unchanged and let WKWebView handle it.
    // Remote http:// images may fail due to WKWebView mixed-content restrictions.
    return url
  }
}

export const tauriMediaResolver = new TauriMediaResolver()
