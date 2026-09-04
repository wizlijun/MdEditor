import type { CanvasClipboardPayload } from './model'

interface CanvasClipboardSnapshot {
  payload: CanvasClipboardPayload
  plainText: string
}

let current: CanvasClipboardSnapshot | null = null

export function rememberCanvasClipboard(payload: CanvasClipboardPayload, plainText: string): void {
  current = { payload, plainText }
}

export function recallCanvasClipboard(plainText?: string): CanvasClipboardPayload | null {
  if (!current) return null
  if (plainText !== undefined && plainText !== current.plainText) return null
  return current.payload
}

export function clearCanvasClipboard(): void {
  current = null
}
