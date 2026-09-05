import type { DocumentRevision } from '../lib/cdr/core'
import type { AppliedChange, OperationBatch, ReplaceBlockOperation } from '../lib/cdr/operation'

export type EditorSnapshot = DocumentRevision
export type LocalOperationBatch = OperationBatch
export type { AppliedChange, ReplaceBlockOperation }

export type EditorCommand =
  | { kind: 'undo' | 'redo' | 'paragraph' | 'bold' | 'italic' | 'strikethrough' | 'code' | 'highlight'
      | 'blockquote' | 'bullet-list' | 'ordered-list' | 'task-list' | 'indent' | 'outdent'
      | 'code-block' | 'horizontal-rule' | 'table' | 'table.add-row' | 'table.delete-row'
      | 'table.add-column' | 'table.delete-column' | 'table.next-cell' | 'table.previous-cell' | 'unlink' }
  | { kind: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6 }
  | { kind: 'link'; href: string; text?: string }
  | { kind: 'image'; src: string; alt?: string }

export interface EditorIdProvider {
  requestId(): string
  operationId(): string
  blockId?(): string
}

export type StructuralCommand =
  | { kind: 'block.insert-after'; blockId: string; content: string }
  | { kind: 'block.delete'; blockId: string }
  | { kind: 'block.move-up' | 'block.move-down'; blockId: string }

export interface ChangeError {
  code: 'stale-base' | 'invalid-operation' | 'unsupported-structure' | 'remote-base-mismatch' | 'persistence-failed'
  message: string
  changeId?: string
}

export type SurfaceUpdate =
  | { kind: 'ack-local'; requestId: string; authoritative: EditorSnapshot; includedChangeIds: readonly string[] }
  | { kind: 'apply-remote'; change: AppliedChange }
  | { kind: 'proposal-stored'; requestId: string; changeSetId: string; authoritative: EditorSnapshot; includedChangeIds: readonly string[] }
  | { kind: 'reject-local'; requestId: string; reason: ChangeError; authoritative: EditorSnapshot; includedChangeIds: readonly string[] }
  | { kind: 'resync'; snapshot: EditorSnapshot; includedChangeIds: readonly string[] }

export interface EditorSurfaceState {
  dirty: boolean
  saving: boolean
  readOnly: boolean
  error: ChangeError | null
  selectedBlockId: string | null
}

export interface DecorationItem {
  blockId: string
  kind: 'activity' | 'proposal' | 'assessment-outdated'
  label: string
}

export interface EditorSurface {
  reconcile(update: SurfaceUpdate): Promise<void>
  observeLocalOperations(listener: (batch: LocalOperationBatch) => void): () => void
  executeStructuralCommand(command: StructuralCommand): boolean
  executeCommand(command: EditorCommand): boolean
  canExecuteCommand(command: EditorCommand): boolean
  observeState(listener: (state: EditorSurfaceState) => void): () => void
  flush(): Promise<void>
  getDraftMarkdown(): string
  retryPending(): boolean
  discardDraft(): void
  restoreRevision(revision: EditorSnapshot): boolean
  selectedBlockId(): string | null
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

export interface MountDocumentEditorOptions {
  snapshot: EditorSnapshot
  ids: EditorIdProvider
  readOnly?: boolean
  baseDir?: string
  placeholder?: string
  localChangeDebounceMs?: number
  onBlockedStructuralEdit?: () => void
  onResyncRequired?: (reason: ChangeError) => void
}

export type MountDocumentEditor = (container: HTMLElement, opts: MountDocumentEditorOptions) => Promise<MountedDocumentEditor>
