import { invoke } from '@tauri-apps/api/core'

export interface CanvasDiskRevision {
  mtimeNs: string
  size: number
  sha256: string
}

export type CanvasExpectedDiskState =
  | { kind: 'missing' }
  | { kind: 'present'; revision: CanvasDiskRevision }

export interface CanvasOpenResult {
  text: string
  revision: CanvasDiskRevision
  requestedPath: string
  canonicalPath: string
}

export type CanvasProbeResult =
  | { kind: 'missing'; requestedPath: string; canonicalPath: string }
  | { kind: 'present'; revision: CanvasDiskRevision; requestedPath: string; canonicalPath: string }

export interface CanvasSaveResult {
  revision: CanvasDiskRevision
  canonicalPath: string
}

export interface CanvasDocumentError {
  kind: 'invalidPath' | 'notFound' | 'tooLarge' | 'invalidUtf8' | 'unstableRead' | 'conflict' | 'io'
  message: string
  expected?: CanvasExpectedDiskState
  actual?: CanvasExpectedDiskState
  canonicalPath?: string
  limitBytes?: number
  actualBytes?: number
}

export function canvasDocumentOpen(path: string): Promise<CanvasOpenResult> {
  return invoke<CanvasOpenResult>('canvas_document_open', { path })
}

export function canvasDocumentProbe(path: string): Promise<CanvasProbeResult> {
  return invoke<CanvasProbeResult>('canvas_document_probe', { path })
}

export function canvasDocumentCreate(path: string, text: string): Promise<CanvasSaveResult> {
  return invoke<CanvasSaveResult>('canvas_document_create', { path, text })
}

export function canvasDocumentSave(
  path: string,
  text: string,
  revision: CanvasDiskRevision,
  force = false,
): Promise<CanvasSaveResult> {
  return invoke<CanvasSaveResult>('canvas_document_save', {
    path,
    text,
    expected: { kind: 'present', revision } satisfies CanvasExpectedDiskState,
    force,
  })
}

export function canvasMtimeMs(revision: CanvasDiskRevision): number {
  try { return Number(BigInt(revision.mtimeNs) / 1_000_000n) }
  catch { return Date.now() }
}

export function asCanvasDocumentError(error: unknown): CanvasDocumentError | null {
  if (!error || typeof error !== 'object') return null
  const candidate = error as Partial<CanvasDocumentError>
  return typeof candidate.kind === 'string' && typeof candidate.message === 'string'
    ? candidate as CanvasDocumentError
    : null
}
