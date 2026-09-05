/**
 * Domain-neutral CDR body-operation protocol.
 *
 * This module is the single wire-shape definition used by the Editor Kit,
 * Core, and profile implementations. It deliberately contains no editor,
 * persistence, Vault, or MEMORY concepts.
 */

export interface ExistingBlockTarget {
  readonly blockId: string
  readonly expectedBlockRevision: string
}

/** A gap is valid only while its neighbours remain adjacent; null/null denotes an empty working list. */
export interface BlockGapTarget {
  readonly leftBlockId: string | null
  readonly rightBlockId: string | null
}

export interface ReplaceBlockOperation {
  readonly kind: 'block.replace'
  readonly operationId: string
  readonly target: ExistingBlockTarget
  readonly payload: { readonly content: string }
}

export interface InsertBlockOperation {
  readonly kind: 'block.insert'
  readonly operationId: string
  readonly target: BlockGapTarget
  readonly payload: {
    readonly candidateBlockId: string
    readonly content: string
    readonly restoreFrom?: { readonly revisionId: string; readonly blockId: string }
  }
}

export interface MoveBlockOperation {
  readonly kind: 'block.move'
  readonly operationId: string
  readonly target: ExistingBlockTarget
  readonly payload: {
    readonly source: BlockGapTarget
    readonly destination: BlockGapTarget
  }
}

export interface DeleteBlockOperation {
  readonly kind: 'block.delete'
  readonly operationId: string
  readonly target: ExistingBlockTarget
  readonly payload: Readonly<Record<string, never>>
}

export type Operation = ReplaceBlockOperation | InsertBlockOperation | DeleteBlockOperation | MoveBlockOperation

export interface OperationBatch {
  readonly requestId: string
  readonly documentId: string
  readonly baseRevisionId: string
  /** Ordered and atomic. Gaps address preceding results; existing versions address the submitted base. */
  readonly operations: readonly Operation[]
}

export interface AppliedChange {
  readonly changeId: string
  readonly originRequestId?: string
  readonly baseRevisionId: string
  readonly revisionId: string
  /** One final revision per replace/insert output ID; delete and move have no entry. */
  readonly blockRevisions: Readonly<Record<string, string>>
  readonly operations: readonly Operation[]
}

export interface ContentWrite {
  readonly blockId: string
  readonly content: string
}

export type OperationProtocolFailure = (path: string, message: string) => never

export class OperationProtocolError extends Error {
  readonly code = 'CDR_OPERATION_INVALID'

  constructor(path: string, message: string) {
    super(`CDR_OPERATION_INVALID: ${path} ${message}`)
    this.name = 'OperationProtocolError'
  }
}

const defaultFailure: OperationProtocolFailure = (path, message) => {
  throw new OperationProtocolError(path, message)
}

function record(value: unknown, path: string, fail: OperationProtocolFailure): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) fail(path, 'must be an object')
  return value as Record<string, unknown>
}

function exactKeys(
  value: Record<string, unknown>,
  path: string,
  required: readonly string[],
  fail: OperationProtocolFailure,
  optional: readonly string[] = [],
): void {
  const allowed = new Set([...required, ...optional])
  for (const key of required) {
    if (!Object.prototype.hasOwnProperty.call(value, key)) fail(`${path}.${key}`, 'is required')
  }
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) fail(`${path}.${key}`, 'is not allowed by this schema')
  }
}

function stringValue(
  value: unknown,
  path: string,
  fail: OperationProtocolFailure,
  allowEmpty = false,
): string {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) {
    fail(path, allowEmpty ? 'must be a string' : 'must be a non-empty string')
  }
  return value as string
}

function nullableString(value: unknown, path: string, fail: OperationProtocolFailure): string | null {
  if (value === null) return null
  return stringValue(value, path, fail)
}

function unique(values: readonly string[], path: string, fail: OperationProtocolFailure): void {
  const seen = new Set<string>()
  for (const value of values) {
    if (seen.has(value)) fail(path, `contains duplicate ${value}`)
    seen.add(value)
  }
}

function parseExistingTarget(
  value: unknown,
  path: string,
  fail: OperationProtocolFailure,
): ExistingBlockTarget {
  const item = record(value, path, fail)
  exactKeys(item, path, ['blockId', 'expectedBlockRevision'], fail)
  return {
    blockId: stringValue(item.blockId, `${path}.blockId`, fail),
    expectedBlockRevision: stringValue(item.expectedBlockRevision, `${path}.expectedBlockRevision`, fail),
  }
}

function parseGapTarget(value: unknown, path: string, fail: OperationProtocolFailure): BlockGapTarget {
  const item = record(value, path, fail)
  exactKeys(item, path, ['leftBlockId', 'rightBlockId'], fail)
  const target = {
    leftBlockId: nullableString(item.leftBlockId, `${path}.leftBlockId`, fail),
    rightBlockId: nullableString(item.rightBlockId, `${path}.rightBlockId`, fail),
  }
  if (target.leftBlockId !== null && target.leftBlockId === target.rightBlockId) {
    fail(path, 'must identify two different neighbouring blocks')
  }
  return target
}

export function parseOperation(
  value: unknown,
  path = 'operation',
  fail: OperationProtocolFailure = defaultFailure,
): Operation {
  const item = record(value, path, fail)
  exactKeys(item, path, ['kind', 'operationId', 'target', 'payload'], fail)
  const operationId = stringValue(item.operationId, `${path}.operationId`, fail)
  const payload = record(item.payload, `${path}.payload`, fail)
  switch (item.kind) {
    case 'block.replace':
      exactKeys(payload, `${path}.payload`, ['content'], fail)
      return {
        kind: 'block.replace',
        operationId,
        target: parseExistingTarget(item.target, `${path}.target`, fail),
        payload: { content: stringValue(payload.content, `${path}.payload.content`, fail, true) },
      }
    case 'block.insert': {
      exactKeys(payload, `${path}.payload`, ['candidateBlockId', 'content'], fail, ['restoreFrom'])
      let restoreFrom: InsertBlockOperation['payload']['restoreFrom']
      if (payload.restoreFrom !== undefined) {
        const source = record(payload.restoreFrom, `${path}.payload.restoreFrom`, fail)
        exactKeys(source, `${path}.payload.restoreFrom`, ['revisionId', 'blockId'], fail)
        restoreFrom = {
          revisionId: stringValue(source.revisionId, `${path}.payload.restoreFrom.revisionId`, fail),
          blockId: stringValue(source.blockId, `${path}.payload.restoreFrom.blockId`, fail),
        }
      }
      return {
        kind: 'block.insert',
        operationId,
        target: parseGapTarget(item.target, `${path}.target`, fail),
        payload: {
          candidateBlockId: stringValue(payload.candidateBlockId, `${path}.payload.candidateBlockId`, fail),
          content: stringValue(payload.content, `${path}.payload.content`, fail, true),
          ...(restoreFrom ? { restoreFrom } : {}),
        },
      }
    }
    case 'block.move':
      exactKeys(payload, `${path}.payload`, ['source', 'destination'], fail)
      return {
        kind: 'block.move',
        operationId,
        target: parseExistingTarget(item.target, `${path}.target`, fail),
        payload: {
          source: parseGapTarget(payload.source, `${path}.payload.source`, fail),
          destination: parseGapTarget(payload.destination, `${path}.payload.destination`, fail),
        },
      }
    case 'block.delete':
      exactKeys(payload, `${path}.payload`, [], fail)
      return {
        kind: 'block.delete',
        operationId,
        target: parseExistingTarget(item.target, `${path}.target`, fail),
        payload: {},
      }
    default:
      return fail(`${path}.kind`, 'must be block.replace, block.insert, block.move, or block.delete')
  }
}

export function operationContentWrites(operation: Operation): readonly ContentWrite[] {
  switch (operation.kind) {
    case 'block.replace':
      return [{ blockId: operation.target.blockId, content: operation.payload.content }]
    case 'block.insert':
      return [{ blockId: operation.payload.candidateBlockId, content: operation.payload.content }]
    case 'block.delete':
    case 'block.move':
      return []
  }
}

export function operationExistingTargetId(operation: Operation): string | null {
  return operation.kind === 'block.insert' ? null : operation.target.blockId
}

export function parseOperationBatch(
  value: unknown,
  path = 'batch',
  fail: OperationProtocolFailure = defaultFailure,
): OperationBatch {
  const item = record(value, path, fail)
  exactKeys(item, path, ['requestId', 'documentId', 'baseRevisionId', 'operations'], fail)
  if (!Array.isArray(item.operations)) fail(`${path}.operations`, 'must be an array')
  const operations = (item.operations as unknown[])
    .map((operation, index) => parseOperation(operation, `${path}.operations[${index}]`, fail))
  unique(operations.map((operation) => operation.operationId), `${path}.operations operationIds`, fail)
  unique(
    operations.flatMap((operation) => {
      if (operation.kind === 'block.move') return []
      const target = operationExistingTargetId(operation)
      return target === null ? [] : [target]
    }),
    `${path}.operations existing target blockIds`,
    fail,
  )
  unique(operations.filter((operation) => operation.kind === 'block.insert')
    .map((operation) => operation.payload.candidateBlockId), `${path}.operations inserted blockIds`, fail)
  unique(operations.filter((operation) => operation.kind === 'block.move')
    .map((operation) => operation.target.blockId), `${path}.operations moved blockIds`, fail)
  return {
    requestId: stringValue(item.requestId, `${path}.requestId`, fail),
    documentId: stringValue(item.documentId, `${path}.documentId`, fail),
    baseRevisionId: stringValue(item.baseRevisionId, `${path}.baseRevisionId`, fail),
    operations,
  }
}

/** Stable JSON used by every idempotency boundary for this protocol version. */
export function canonicalOperationBatch(batch: OperationBatch): string {
  return JSON.stringify(parseOperationBatch(batch))
}
