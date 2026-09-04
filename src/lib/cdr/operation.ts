/**
 * Domain-neutral CDR internal operation protocol (Stage 1A v0).
 *
 * This module is the single definition used by the Editor Kit and by profile
 * implementations. It deliberately contains no editor, persistence, Vault or
 * MEMORY concepts.
 */

export interface ReplaceBlockOperation {
  readonly kind: 'block.replace'
  readonly operationId: string
  readonly blockId: string
  readonly expectedBlockRevision: string
  readonly markdown: string
}

export type Operation = ReplaceBlockOperation

export interface OperationBatch {
  readonly requestId: string
  readonly baseRevisionId: string
  readonly operations: readonly Operation[]
}

export interface AppliedChange {
  readonly changeId: string
  readonly originRequestId?: string
  readonly baseRevisionId: string
  readonly revisionId: string
  readonly blockRevisions: Readonly<Record<string, string>>
  readonly operations: readonly Operation[]
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
): void {
  const allowed = new Set(required)
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

function unique(values: readonly string[], path: string, fail: OperationProtocolFailure): void {
  const seen = new Set<string>()
  for (const value of values) {
    if (seen.has(value)) fail(path, `contains duplicate ${value}`)
    seen.add(value)
  }
}

export function parseOperation(
  value: unknown,
  path = 'operation',
  fail: OperationProtocolFailure = defaultFailure,
): Operation {
  const item = record(value, path, fail)
  exactKeys(item, path, ['kind', 'operationId', 'blockId', 'expectedBlockRevision', 'markdown'], fail)
  if (item.kind !== 'block.replace') fail(`${path}.kind`, 'must be block.replace')
  return {
    kind: 'block.replace',
    operationId: stringValue(item.operationId, `${path}.operationId`, fail),
    blockId: stringValue(item.blockId, `${path}.blockId`, fail),
    expectedBlockRevision: stringValue(item.expectedBlockRevision, `${path}.expectedBlockRevision`, fail),
    markdown: stringValue(item.markdown, `${path}.markdown`, fail, true),
  }
}

export function parseOperationBatch(
  value: unknown,
  path = 'batch',
  fail: OperationProtocolFailure = defaultFailure,
): OperationBatch {
  const item = record(value, path, fail)
  exactKeys(item, path, ['requestId', 'baseRevisionId', 'operations'], fail)
  if (!Array.isArray(item.operations)) fail(`${path}.operations`, 'must be an array')
  const operations = (item.operations as unknown[])
    .map((operation, index) => parseOperation(operation, `${path}.operations[${index}]`, fail))
  unique(operations.map((operation) => operation.operationId), `${path}.operations operationIds`, fail)
  unique(operations.map((operation) => operation.blockId), `${path}.operations blockIds`, fail)
  return {
    requestId: stringValue(item.requestId, `${path}.requestId`, fail),
    baseRevisionId: stringValue(item.baseRevisionId, `${path}.baseRevisionId`, fail),
    operations,
  }
}

/** Stable JSON used by every idempotency boundary for this protocol version. */
export function canonicalOperationBatch(batch: OperationBatch): string {
  return JSON.stringify(parseOperationBatch(batch))
}
