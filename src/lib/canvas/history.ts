import { encodeJsonCanvas } from './json-canvas'
import { type CanvasDocument, cloneCanvasDocument } from './types'

export interface CanvasPatch {
  readonly label: string
  readonly before: CanvasDocument
  readonly after: CanvasDocument
  readonly estimatedBytes: number
}

export interface CanvasHistoryOptions {
  maxEntries?: number
  maxBytes?: number
}

export function createCanvasPatch(
  label: string,
  before: CanvasDocument,
  after: CanvasDocument,
): CanvasPatch | undefined {
  const beforeText = encodeJsonCanvas(before, { indent: 0 })
  const afterText = encodeJsonCanvas(after, { indent: 0 })
  if (beforeText === afterText) return undefined
  return {
    label,
    before: cloneCanvasDocument(before),
    after: cloneCanvasDocument(after),
    estimatedBytes: (beforeText.length + afterText.length) * 2,
  }
}

export function applyCanvasPatch(patch: CanvasPatch, direction: 'undo' | 'redo'): CanvasDocument {
  return cloneCanvasDocument(direction === 'undo' ? patch.before : patch.after)
}

/** Document history owner. Save does not clear it; external reload must call clear(). */
export class CanvasHistory {
  private readonly maxEntries: number
  private readonly maxBytes: number
  private undoStack: CanvasPatch[] = []
  private redoStack: CanvasPatch[] = []
  private retainedBytes = 0

  constructor(options: CanvasHistoryOptions = {}) {
    this.maxEntries = options.maxEntries ?? 100
    this.maxBytes = options.maxBytes ?? 20 * 1024 * 1024
    if (this.maxEntries < 0 || this.maxBytes < 0) throw new RangeError('Canvas history 限额不能为负数')
  }

  get canUndo(): boolean { return this.undoStack.length > 0 }
  get canRedo(): boolean { return this.redoStack.length > 0 }
  get undoLabel(): string | undefined { return this.undoStack.at(-1)?.label }
  get redoLabel(): string | undefined { return this.redoStack.at(-1)?.label }
  get size(): number { return this.undoStack.length }

  record(label: string, before: CanvasDocument, after: CanvasDocument): CanvasPatch | undefined {
    const patch = createCanvasPatch(label, before, after)
    if (!patch) return undefined
    for (const redo of this.redoStack) this.retainedBytes -= redo.estimatedBytes
    this.redoStack = []
    this.undoStack.push(patch)
    this.retainedBytes += patch.estimatedBytes
    this.trim()
    return patch
  }

  undo(): CanvasDocument | undefined {
    const patch = this.undoStack.pop()
    if (!patch) return undefined
    this.redoStack.push(patch)
    return applyCanvasPatch(patch, 'undo')
  }

  redo(): CanvasDocument | undefined {
    const patch = this.redoStack.pop()
    if (!patch) return undefined
    this.undoStack.push(patch)
    return applyCanvasPatch(patch, 'redo')
  }

  clear(): void {
    this.undoStack = []
    this.redoStack = []
    this.retainedBytes = 0
  }

  private trim(): void {
    while (this.undoStack.length > this.maxEntries || this.retainedBytes > this.maxBytes) {
      const dropped = this.undoStack.shift()
      if (!dropped) break
      this.retainedBytes -= dropped.estimatedBytes
    }
  }
}
