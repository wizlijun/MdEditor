import { bridge } from '../bridge'
import { GovernedRevisionChangedError } from '../../../../../src/lib/cdr/governance'
import {
  InMemoryDocumentSession,
  InvalidSessionStateError,
  uuidIds,
  type Assessment,
  type AuditEvent,
  type DocumentRevision,
  type DocumentSessionState,
  type IdProvider,
  type OperationBatch,
  type Proposal,
  type SubmitResult,
} from './session'

export const CDR_REPOSITORY_LOAD_METHOD = 'host.cdr.repository.v1.load' as const
export const CDR_REPOSITORY_COMMIT_METHOD = 'host.cdr.repository.v1.commit' as const

export interface StoredAggregate {
  generation: number
  /** Untrusted until InMemoryDocumentSession.fromState validates it. */
  aggregate: unknown
}

export interface AggregateCommit {
  documentId: string
  expectedGeneration: number
  aggregate: DocumentSessionState
}

export type AggregateCommitResult =
  | { kind: 'committed'; generation: number }
  | { kind: 'conflict'; current: StoredAggregate }

/** Minimal durable CAS boundary. It intentionally exposes no file layout. */
export interface AggregateStore {
  load(documentId: string): Promise<StoredAggregate | undefined>
  commit(command: AggregateCommit): Promise<AggregateCommitResult>
}

export type RepositoryRequest = (method: string, params?: unknown) => Promise<unknown>

export class RepositoryIOError extends Error {
  readonly code = 'CDR_REPOSITORY_IO'
  readonly causeValue: unknown

  constructor(message: string, causeValue?: unknown) {
    super(`CDR_REPOSITORY_IO: ${message}`)
    this.name = 'RepositoryIOError'
    this.causeValue = causeValue
  }
}

export class RepositoryConflictError extends Error {
  readonly code = 'CDR_REPOSITORY_CONFLICT'

  constructor(
    readonly currentSnapshot: DocumentRevision,
    readonly currentGeneration: number,
  ) {
    super('CDR_REPOSITORY_CONFLICT: aggregate changed; current committed state was loaded')
    this.name = 'RepositoryConflictError'
  }
}

export class RepositoryOutcomeUnknownError extends Error {
  readonly code = 'CDR_REPOSITORY_OUTCOME_UNKNOWN'
  readonly causeValue: unknown

  constructor(message: string, causeValue?: unknown) {
    super(`CDR_REPOSITORY_OUTCOME_UNKNOWN: ${message}`)
    this.name = 'RepositoryOutcomeUnknownError'
    this.causeValue = causeValue
  }
}

/** A lower storage layer has proved writes unsafe (for example external drift). */
export class RepositoryWriteBlockedError extends Error {
  readonly code = 'CDR_REPOSITORY_WRITE_BLOCKED'

  constructor(message: string) {
    super(`CDR_REPOSITORY_WRITE_BLOCKED: ${message}`)
    this.name = 'RepositoryWriteBlockedError'
  }
}

function responseRecord(value: unknown, method: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new RepositoryIOError(`${method} returned a non-object response`)
  }
  return value as Record<string, unknown>
}

function responseGeneration(value: unknown, method: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    throw new RepositoryIOError(`${method} returned an invalid generation`)
  }
  return value as number
}

function exactResponse(value: Record<string, unknown>, method: string, keys: readonly string[]): void {
  const actual = Object.keys(value).sort()
  const expected = [...keys].sort()
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new RepositoryIOError(`${method} returned unexpected fields`)
  }
}

function equivalentState(left: DocumentSessionState, right: DocumentSessionState): boolean {
  const canonical = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(canonical)
    if (value === null || typeof value !== 'object') return value
    return Object.fromEntries(Object.entries(value as Record<string, unknown>)
      .sort(([leftKey], [rightKey]) => leftKey.localeCompare(rightKey))
      .map(([key, item]) => [key, canonical(item)]))
  }
  return JSON.stringify(canonical(left)) === JSON.stringify(canonical(right))
}

/**
 * Wire contract:
 * load({document_id}) -> {kind:'missing'} |
 *                        {kind:'loaded', generation, aggregate}
 * commit({document_id, expected_generation, aggregate}) ->
 *                        {kind:'committed', generation} |
 *                        {kind:'conflict', current:{generation, aggregate}}
 */
export class HostBridgeAggregateStore implements AggregateStore {
  constructor(
    private readonly request: RepositoryRequest = (method, params) => bridge().request(method, params),
  ) {}

  async load(documentId: string): Promise<StoredAggregate | undefined> {
    const response = responseRecord(
      await this.request(CDR_REPOSITORY_LOAD_METHOD, { document_id: documentId }),
      CDR_REPOSITORY_LOAD_METHOD,
    )
    if (response.kind === 'missing') {
      exactResponse(response, CDR_REPOSITORY_LOAD_METHOD, ['kind'])
      return undefined
    }
    if (response.kind !== 'loaded' || !Object.prototype.hasOwnProperty.call(response, 'aggregate')) {
      throw new RepositoryIOError(`${CDR_REPOSITORY_LOAD_METHOD} returned an unsupported response`)
    }
    exactResponse(response, CDR_REPOSITORY_LOAD_METHOD, ['kind', 'generation', 'aggregate'])
    const generation = responseGeneration(response.generation, CDR_REPOSITORY_LOAD_METHOD)
    if (generation === 0) throw new RepositoryIOError(`${CDR_REPOSITORY_LOAD_METHOD} returned generation zero for loaded state`)
    return {
      generation,
      aggregate: response.aggregate,
    }
  }

  async commit(command: AggregateCommit): Promise<AggregateCommitResult> {
    const response = responseRecord(
      await this.request(CDR_REPOSITORY_COMMIT_METHOD, {
        document_id: command.documentId,
        expected_generation: command.expectedGeneration,
        aggregate: command.aggregate,
      }),
      CDR_REPOSITORY_COMMIT_METHOD,
    )
    if (response.kind === 'conflict') {
      exactResponse(response, CDR_REPOSITORY_COMMIT_METHOD, ['kind', 'current'])
      const current = responseRecord(response.current, `${CDR_REPOSITORY_COMMIT_METHOD}.current`)
      exactResponse(current, `${CDR_REPOSITORY_COMMIT_METHOD}.current`, ['generation', 'aggregate'])
      if (!Object.prototype.hasOwnProperty.call(current, 'aggregate')) {
        throw new RepositoryIOError(`${CDR_REPOSITORY_COMMIT_METHOD} conflict omitted current.aggregate`)
      }
      return {
        kind: 'conflict',
        current: {
          generation: responseGeneration(current.generation, CDR_REPOSITORY_COMMIT_METHOD),
          aggregate: current.aggregate,
        },
      }
    }
    if (response.kind !== 'committed') {
      throw new RepositoryIOError(`${CDR_REPOSITORY_COMMIT_METHOD} returned an unsupported response`)
    }
    exactResponse(response, CDR_REPOSITORY_COMMIT_METHOD, ['kind', 'generation'])
    const generation = responseGeneration(response.generation, CDR_REPOSITORY_COMMIT_METHOD)
    if (generation !== command.expectedGeneration + 1) {
      throw new RepositoryIOError(`${CDR_REPOSITORY_COMMIT_METHOD} returned a non-successor generation`)
    }
    return {
      kind: 'committed',
      generation,
    }
  }
}

export type PersistentSessionOpenKind = 'created' | 'restored'

function repositoryIO(action: string, error: unknown): RepositoryIOError {
  return error instanceof RepositoryIOError
    ? error
    : new RepositoryIOError(`${action} failed`, error)
}

/**
 * Copy-on-write persistent facade around the state machine.
 * Calls on one instance are serialized. A failed durable write never exposes
 * its candidate state; after CAS conflict, the latest valid committed state is
 * installed and the initiating direct operation still fails. Governed calls
 * receive a port-level head-changed signal so the Application Service can
 * re-authorize and retry against the newly installed aggregate.
 */
export class PersistentDocumentSession {
  #writeTail: Promise<void> = Promise.resolve()
  #writeFailure: RepositoryOutcomeUnknownError | RepositoryWriteBlockedError | null = null

  private constructor(
    private readonly store: AggregateStore,
    private session: InMemoryDocumentSession,
    private generation: number,
    readonly openKind: PersistentSessionOpenKind,
    private readonly ids: IdProvider,
  ) {}

  static async open(
    initial: DocumentRevision,
    ids: IdProvider = uuidIds(),
    store: AggregateStore = new HostBridgeAggregateStore(),
  ): Promise<PersistentDocumentSession> {
    // Validate the caller-provided seed even when a stored aggregate exists.
    const seed = new InMemoryDocumentSession(initial, ids)
    let stored: StoredAggregate | undefined
    try {
      stored = await store.load(initial.documentId)
    } catch (error) {
      throw repositoryIO('load aggregate', error)
    }

    if (stored) {
      const restored = InMemoryDocumentSession.fromState(stored.aggregate, ids)
      if (restored.snapshot().documentId !== initial.documentId) {
        throw new InvalidSessionStateError('state.head.documentId does not match the requested document')
      }
      return new PersistentDocumentSession(store, restored, stored.generation, 'restored', ids)
    }

    let created: AggregateCommitResult
    try {
      created = await store.commit({
        documentId: initial.documentId,
        expectedGeneration: 0,
        aggregate: seed.exportState(),
      })
    } catch (error) {
      let recovered: StoredAggregate | undefined
      try {
        recovered = await store.load(initial.documentId)
      } catch (recoveryError) {
        throw new RepositoryOutcomeUnknownError('create failed and its durable outcome could not be read', {
          commitError: error,
          recoveryError,
        })
      }
      if (!recovered) throw repositoryIO('create aggregate', error)
      const restored = InMemoryDocumentSession.fromState(recovered.aggregate, ids)
      if (restored.snapshot().documentId !== initial.documentId) {
        throw new InvalidSessionStateError('state.head.documentId does not match the requested document')
      }
      return new PersistentDocumentSession(
        store,
        restored,
        recovered.generation,
        equivalentState(restored.exportState(), seed.exportState()) ? 'created' : 'restored',
        ids,
      )
    }
    if (created.kind === 'committed') {
      return new PersistentDocumentSession(store, seed, created.generation, 'created', ids)
    }

    // Another opener won creation. The CAS response already carries the exact
    // committed aggregate, so no second read can race with this recovery.
    const restored = InMemoryDocumentSession.fromState(created.current.aggregate, ids)
    if (restored.snapshot().documentId !== initial.documentId) {
      throw new InvalidSessionStateError('state.head.documentId does not match the requested document')
    }
    return new PersistentDocumentSession(store, restored, created.current.generation, 'restored', ids)
  }

  get isNew(): boolean {
    return this.openKind === 'created'
  }

  snapshot(): DocumentRevision {
    return this.session.snapshot()
  }

  revisionHistory(): readonly DocumentRevision[] {
    return this.session.revisionHistory()
  }

  proposals(): readonly Proposal[] {
    return this.session.proposals()
  }

  proposal(changeSetId: string): Proposal | undefined {
    return this.session.proposal(changeSetId)
  }

  assessments(): readonly Assessment[] {
    return this.session.assessments()
  }

  audit(): readonly AuditEvent[] {
    return this.session.audit()
  }

  assessmentIsOutdated(assessment: Assessment): boolean {
    return this.session.assessmentIsOutdated(assessment)
  }

  async submit(batch: OperationBatch, actorId: string, governedRevisionId?: string): Promise<SubmitResult> {
    return this.mutate((candidate) => candidate.submit(batch, actorId, governedRevisionId), governedRevisionId)
  }

  async propose(batch: OperationBatch, actorId: string, governedRevisionId?: string): Promise<Proposal> {
    return this.mutate((candidate) => candidate.propose(batch, actorId, governedRevisionId), governedRevisionId)
  }

  async decideProposal(
    changeSetId: string,
    decision: 'accept' | 'reject',
    actorId: string,
    governedRevisionId?: string,
  ): Promise<SubmitResult | null> {
    return this.mutate(
      (candidate) => candidate.decideProposal(changeSetId, decision, actorId, governedRevisionId),
      governedRevisionId,
    )
  }

  async assess(
    blockId: string,
    actorId: string,
    conclusion: Assessment['conclusion'],
    governedRevisionId?: string,
  ): Promise<Assessment> {
    return this.mutate(
      (candidate) => candidate.assess(blockId, actorId, conclusion, governedRevisionId),
      governedRevisionId,
    )
  }

  private mutate<T>(
    transition: (candidate: InMemoryDocumentSession) => T | Promise<T>,
    governedRevisionId?: string,
  ): Promise<T> {
    const run = this.#writeTail.then(async () => {
      if (this.#writeFailure) throw this.#writeFailure
      const before = this.session.exportState()
      const candidate = InMemoryDocumentSession.fromState(before, this.ids)
      const result = await transition(candidate)
      const after = candidate.exportState()
      if (JSON.stringify(after) === JSON.stringify(before)) return result

      let committed: AggregateCommitResult
      try {
        committed = await this.store.commit({
          documentId: after.head.documentId,
          expectedGeneration: this.generation,
          aggregate: after,
        })
      } catch (error) {
        if (error instanceof RepositoryWriteBlockedError) {
          this.#writeFailure = error
          throw error
        }
        const recovered = await this.recoverCommitOutcome(before, after, error)
        if (recovered === 'candidate') return result
        throw repositoryIO('commit aggregate', error)
      }
      if (committed.kind === 'committed') {
        this.session = candidate
        this.generation = committed.generation
        return result
      }

      try {
        if (committed.current.generation <= this.generation) {
          throw new InvalidSessionStateError('CAS conflict current generation must advance')
        }
        this.installCurrent(after.head.documentId, committed.current)
      } catch (error) {
        throw this.blockWrites('CAS conflict returned an invalid current aggregate', error)
      }
      throw new RepositoryConflictError(this.snapshot(), this.generation)
    })
    this.#writeTail = run.then(() => undefined, () => undefined)
    return run.catch((error) => {
      if (governedRevisionId !== undefined && error instanceof RepositoryConflictError) {
        throw new GovernedRevisionChangedError()
      }
      throw error
    })
  }

  private installCurrent(documentId: string, current: StoredAggregate): void {
    const restored = InMemoryDocumentSession.fromState(current.aggregate, this.ids)
    if (restored.snapshot().documentId !== documentId) {
      throw new InvalidSessionStateError('state.head.documentId does not match the requested document')
    }
    this.session = restored
    this.generation = current.generation
  }

  private async recoverCommitOutcome(
    before: DocumentSessionState,
    candidate: DocumentSessionState,
    commitError: unknown,
  ): Promise<'candidate' | 'previous'> {
    let current: StoredAggregate | undefined
    try {
      current = await this.store.load(candidate.head.documentId)
    } catch (recoveryError) {
      throw this.blockWrites('commit failed and its durable outcome could not be read', {
        commitError,
        recoveryError,
      })
    }
    if (!current) throw this.blockWrites('commit failed and the previous aggregate disappeared', commitError)

    let restored: InMemoryDocumentSession
    try {
      restored = InMemoryDocumentSession.fromState(current.aggregate, this.ids)
      if (restored.snapshot().documentId !== candidate.head.documentId) {
        throw new InvalidSessionStateError('state.head.documentId does not match the requested document')
      }
    } catch (error) {
      throw this.blockWrites('commit failed and the recovered aggregate is invalid', error)
    }

    const recoveredState = restored.exportState()
    if (current.generation > this.generation && equivalentState(recoveredState, candidate)) {
      this.session = restored
      this.generation = current.generation
      return 'candidate'
    }
    if (current.generation === this.generation && equivalentState(recoveredState, before)) {
      return 'previous'
    }
    if (current.generation <= this.generation) {
      throw this.blockWrites('commit recovery returned an inconsistent generation', commitError)
    }

    this.session = restored
    this.generation = current.generation
    throw new RepositoryConflictError(this.snapshot(), this.generation)
  }

  private blockWrites(message: string, causeValue?: unknown): RepositoryOutcomeUnknownError {
    const failure = new RepositoryOutcomeUnknownError(message, causeValue)
    this.#writeFailure = failure
    return failure
  }
}
