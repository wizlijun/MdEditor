import { bridge } from './bridge'
import type {
  AppliedChange,
  DocumentRevision,
  OperationBatch,
  ReplaceBlockOperation,
} from './cdr/session'

export type EditorSnapshot = DocumentRevision
export type LocalOperationBatch = OperationBatch

export type SurfaceUpdate =
  | {
      kind: 'ack-local'
      requestId: string
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | { kind: 'apply-remote'; change: AppliedChange }
  | {
      kind: 'proposal-stored'
      requestId: string
      changeSetId: string
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | {
      kind: 'reject-local'
      requestId: string
      reason: {
        code: 'stale-base' | 'invalid-operation' | 'unsupported-structure' | 'remote-base-mismatch'
        message: string
        changeId?: string
      }
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | { kind: 'resync'; snapshot: EditorSnapshot; includedChangeIds: readonly string[] }

export interface DecorationItem {
  blockId: string
  kind: 'activity' | 'proposal' | 'assessment-outdated'
  label: string
}

export interface EditorSurface {
  reconcile(update: SurfaceUpdate): Promise<void>
  observeLocalOperations(listener: (batch: LocalOperationBatch) => void): () => void
  setReadOnly(value: boolean): void
  destroy(): Promise<void>
}

export interface DecorationHost {
  setLayer(id: string, items: readonly DecorationItem[]): void
  removeLayer(id: string): void
}

export interface MountedDocumentEditor {
  surface: EditorSurface
  decorations: DecorationHost
}

export interface EditorIdProvider {
  requestId(): string
  operationId(): string
}

export interface MountDocumentEditorOptions {
  snapshot: EditorSnapshot
  ids: EditorIdProvider
  readOnly?: boolean
  baseDir?: string
  placeholder?: string
  onBlockedStructuralEdit?: () => void
  onResyncRequired?: (reason: { code: 'remote-base-mismatch'; message: string; changeId?: string }) => void
}

export type MountDocumentEditor = (
  container: HTMLElement,
  opts: MountDocumentEditorOptions,
) => Promise<MountedDocumentEditor>

export function kitV2Url(): string {
  return `plugin://${bridge().pluginId}/__host__/assets/editor-kit-v2.js`
}

export async function loadKitV2(): Promise<MountDocumentEditor> {
  const mod = (await import(/* @vite-ignore */ kitV2Url())) as { mountDocumentEditor?: unknown }
  if (typeof mod.mountDocumentEditor !== 'function') {
    throw new Error('editor-kit-v2.js loaded but exports no mountDocumentEditor')
  }
  return mod.mountDocumentEditor as MountDocumentEditor
}

export function replaceOperation(
  ids: EditorIdProvider,
  blockId: string,
  expectedBlockRevision: string,
  markdown: string,
): ReplaceBlockOperation {
  return { kind: 'block.replace', operationId: ids.operationId(), blockId, expectedBlockRevision, markdown }
}
