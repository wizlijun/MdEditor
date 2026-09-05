import { describe, expect, it, vi } from 'vitest'
import type { ChangePolicyRequest } from '../../../../../src/lib/cdr/governance'
import {
  CDR_REPOSITORY_COMMIT_METHOD,
  CDR_REPOSITORY_LOAD_METHOD,
  HostBridgeAggregateStore,
  PersistentDocumentSession,
  RepositoryConflictError,
  RepositoryIOError,
  RepositoryOutcomeUnknownError,
  type AggregateCommit,
  type AggregateCommitResult,
  type AggregateStore,
  type StoredAggregate,
} from './repository'
import {
  DOCUMENT_SESSION_STATE_SCHEMA,
  InvalidSessionStateError,
  sequentialIds,
  type DocumentRevision,
  type DocumentSessionState,
  type OperationBatch,
} from './session'
import { CdrApplicationService, fixedActorSource } from './application'
import { localMemoryAuthorizer, MEMORY_SELF_PROFILE_DESCRIPTOR, memorySelfProfile } from './profile'

function fixture(): DocumentRevision {
  return {
    documentId: 'document-1',
    revisionId: 'revision-1',
    blocks: [
      { blockId: 'block-a', blockRevision: 'block-a/1', markdown: '# Background' },
      { blockId: 'block-b', blockRevision: 'block-b/1', markdown: 'Keep this constraint.' },
    ],
  }
}

function replace(requestId: string, blockId: string, expectedBlockRevision: string, markdown: string): OperationBatch {
  return {
    requestId,
    documentId: 'document-1',
    baseRevisionId: 'revision-1',
    operations: [{
      kind: 'block.replace',
      operationId: `${requestId}/op`,
      target: { blockId, expectedBlockRevision },
      payload: { content: markdown },
    }],
  }
}

function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

class FakeAggregateStore implements AggregateStore {
  record: StoredAggregate | undefined
  commitCount = 0
  failNextCommit = false
  loseNextCommitReceipt = false
  failNextLoad = false
  private nextVersion = 0

  async load(documentId: string): Promise<StoredAggregate | undefined> {
    if (this.failNextLoad) {
      this.failNextLoad = false
      throw new Error('read unavailable')
    }
    if (!this.record) return undefined
    const aggregate = this.record.aggregate as { head?: { documentId?: string } }
    if (aggregate.head?.documentId !== documentId) return undefined
    return copy(this.record)
  }

  async commit(command: AggregateCommit): Promise<AggregateCommitResult> {
    this.commitCount += 1
    if (this.failNextCommit) {
      this.failNextCommit = false
      throw new Error('disk unavailable')
    }
    const currentGeneration = this.record?.generation ?? 0
    if (command.expectedGeneration !== currentGeneration) {
      return { kind: 'conflict', current: copy(this.record ?? { generation: 0, aggregate: {} }) }
    }
    const generation = ++this.nextVersion
    this.record = { generation, aggregate: copy(command.aggregate) }
    if (this.loseNextCommitReceipt) {
      this.loseNextCommitReceipt = false
      throw new Error('commit receipt lost')
    }
    return { kind: 'committed', generation }
  }
}

describe('PersistentDocumentSession', () => {
  it('persists an Agent proposal and its accepted self change through separate single-CAS transitions', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('governed'), store)
    const agent = new CdrApplicationService(
      'document-1',
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      session,
      fixedActorSource({ kind: 'agent', id: 'organizer' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    const human = new CdrApplicationService(
      'document-1',
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      session,
      fixedActorSource({ kind: 'human', id: 'local' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )

    const proposed = await agent.submit(replace('governed-proposal', 'block-a', 'block-a/1', '# Governed'), 'apply')
    expect(proposed.kind).toBe('proposed')
    expect(store.commitCount).toBe(2)
    expect((store.record?.aggregate as DocumentSessionState).head).toEqual(fixture())
    if (proposed.kind !== 'proposed') throw new Error('expected proposal')

    const accepted = await human.decideProposal(proposed.proposal.changeSetId, 'accept')
    expect(accepted?.kind).toBe('applied')
    expect(store.commitCount).toBe(3)
    const durable = store.record?.aggregate as DocumentSessionState
    expect(durable.head.blocks[0].markdown).toBe('# Governed')
    expect(durable.revisionHistory).toEqual([fixture()])
    expect(durable.proposals[0].status).toBe('applied')
    expect(durable.receipts).toHaveLength(1)
    expect(durable.audit.map((event) => event.action)).toEqual(['proposed', 'applied', 'proposal-applied'])
  })

  it('persists and restores the head, proposals, assessments, receipts, and audit', async () => {
    const store = new FakeAggregateStore()
    const first = await PersistentDocumentSession.open(fixture(), sequentialIds('first'), store)
    expect(first.openKind).toBe('created')
    expect(first.isNew).toBe(true)

    await first.submit(replace('submit', 'block-a', 'block-a/1', '# Persisted'), 'human')
    await first.propose(replace('proposal', 'block-b', 'block-b/1', 'Suggested constraint.'), 'agent')
    await first.assess('block-b', 'verifier', 'verified')

    const reopened = await PersistentDocumentSession.open(fixture(), sequentialIds('reopen'), store)
    expect(reopened.openKind).toBe('restored')
    expect(reopened.isNew).toBe(false)
    expect(reopened.snapshot()).toEqual(first.snapshot())
    expect(reopened.proposals()).toEqual(first.proposals())
    expect(reopened.assessments()).toEqual(first.assessments())
    expect(reopened.audit()).toEqual(first.audit())
  })

  it('preserves idempotency across reopen without committing a duplicate snapshot', async () => {
    const store = new FakeAggregateStore()
    const batch = replace('same-request', 'block-a', 'block-a/1', '# Once')
    const first = await PersistentDocumentSession.open(fixture(), sequentialIds('first'), store)
    const original = await first.submit(batch, 'human')
    const commitsAfterOriginal = store.commitCount

    const reopened = await PersistentDocumentSession.open(fixture(), sequentialIds('reopen'), store)
    const duplicate = await reopened.submit(batch, 'human')

    expect(original.kind).toBe('applied')
    expect(duplicate).toMatchObject({ kind: 'applied', duplicate: true })
    expect(reopened.audit().filter((event) => event.action === 'applied')).toHaveLength(1)
    expect(store.commitCount).toBe(commitsAfterOriginal)
  })

  it('keeps the last committed state readable when an atomic clear fails persistence', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('test'), store)
    const before = session.snapshot()
    store.failNextCommit = true

    const replacement = replace('failed', 'block-a', 'block-a/1', '')
    await expect(session.submit({ ...replacement, operations: [...replacement.operations, {
      kind: 'block.delete', operationId: 'clear-b',
      target: { blockId: 'block-b', expectedBlockRevision: 'block-b/1' }, payload: {},
    }] }, 'human'))
      .rejects.toBeInstanceOf(RepositoryIOError)
    expect(session.snapshot()).toEqual(before)
    expect(session.audit()).toHaveLength(0)
    expect((store.record?.aggregate as DocumentSessionState).head).toEqual(before)
    expect(session.revisionHistory()).toHaveLength(0)
  })

  it('re-reads an ambiguous commit and treats an exact persisted candidate as success', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('test'), store)
    store.loseNextCommitReceipt = true

    await expect(session.submit(replace('ambiguous', 'block-a', 'block-a/1', '# Durable'), 'human'))
      .resolves.toMatchObject({ kind: 'applied', duplicate: false })
    expect(session.snapshot().blocks[0].markdown).toBe('# Durable')
    expect(store.record?.aggregate).toEqual(expect.objectContaining({ head: session.snapshot() }))
  })

  it('blocks later writes when an ambiguous commit cannot be read back', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('test'), store)
    store.loseNextCommitReceipt = true
    store.failNextLoad = true

    await expect(session.submit(replace('unknown', 'block-a', 'block-a/1', '# Maybe durable'), 'human'))
      .rejects.toBeInstanceOf(RepositoryOutcomeUnknownError)
    const commitsAfterUnknown = store.commitCount
    await expect(session.submit(replace('later', 'block-b', 'block-b/1', 'Must not write.'), 'human'))
      .rejects.toBeInstanceOf(RepositoryOutcomeUnknownError)
    expect(store.commitCount).toBe(commitsAfterUnknown)
  })

  it('loads current committed state and fails closed after a CAS conflict', async () => {
    const store = new FakeAggregateStore()
    const winner = await PersistentDocumentSession.open(fixture(), sequentialIds('winner'), store)
    const stale = await PersistentDocumentSession.open(fixture(), sequentialIds('stale'), store)
    await winner.submit(replace('winner-request', 'block-a', 'block-a/1', '# Winner'), 'human-a')

    await expect(stale.submit(replace('stale-request', 'block-b', 'block-b/1', 'Would otherwise rebase.'), 'human-b'))
      .rejects.toBeInstanceOf(RepositoryConflictError)
    expect(stale.snapshot()).toEqual(winner.snapshot())
    expect(stale.snapshot().blocks.find((block) => block.blockId === 'block-b')?.markdown).toBe('Keep this constraint.')
  })

  it('re-governs a safe change after another facade wins the aggregate CAS', async () => {
    const store = new FakeAggregateStore()
    const winner = await PersistentDocumentSession.open(fixture(), sequentialIds('winner'), store)
    const stale = await PersistentDocumentSession.open(fixture(), sequentialIds('stale'), store)
    const governedRevisions: string[] = []
    const evaluate = vi.fn((request: ChangePolicyRequest) => {
      governedRevisions.push(request.snapshot.revisionId)
      return memorySelfProfile.policy!.evaluate(request)
    })
    const app = new CdrApplicationService(
      'document-1',
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      stale,
      fixedActorSource({ kind: 'human', id: 'local' }),
      localMemoryAuthorizer,
      { descriptor: MEMORY_SELF_PROFILE_DESCRIPTOR, policy: { evaluate } },
    )

    await winner.submit(replace('winner-request', 'block-a', 'block-a/1', '# Winner'), 'human:winner')
    const winnerRevision = winner.snapshot().revisionId
    await expect(app.submit(replace('safe-rebase', 'block-b', 'block-b/1', 'Rebased.')))
      .resolves.toMatchObject({ kind: 'applied' })

    expect(governedRevisions).toEqual(['revision-1', winnerRevision])
    expect(evaluate).toHaveBeenCalledTimes(2)
    expect(stale.snapshot().blocks.map((block) => block.markdown)).toEqual(['# Winner', 'Rebased.'])
    expect((store.record?.aggregate as DocumentSessionState).head).toEqual(stale.snapshot())
  })

  it('blocks later writes when a CAS conflict carries an invalid current aggregate', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('test'), store)
    store.record = { generation: 2, aggregate: { schema: 'damaged' } }

    await expect(session.submit(replace('conflict', 'block-a', 'block-a/1', '# Candidate'), 'human'))
      .rejects.toBeInstanceOf(RepositoryOutcomeUnknownError)
    const commitsAfterConflict = store.commitCount
    await expect(session.submit(replace('later', 'block-b', 'block-b/1', 'Must not write.'), 'human'))
      .rejects.toBeInstanceOf(RepositoryOutcomeUnknownError)
    expect(store.commitCount).toBe(commitsAfterConflict)
  })

  it('serializes concurrent writes made through one instance', async () => {
    const store = new FakeAggregateStore()
    const session = await PersistentDocumentSession.open(fixture(), sequentialIds('serial'), store)
    const first = session.submit(replace('one', 'block-a', 'block-a/1', '# One'), 'human')
    const second = session.submit(replace('two', 'block-b', 'block-b/1', 'Two.'), 'human')

    await expect(Promise.all([first, second])).resolves.toMatchObject([
      { kind: 'applied' },
      { kind: 'applied' },
    ])
    expect(session.snapshot().blocks.map((block) => block.markdown)).toEqual(['# One', 'Two.'])
  })

  it('rejects a stored aggregate with a damaged or unsupported schema', async () => {
    const store = new FakeAggregateStore()
    store.record = {
      generation: 1,
      aggregate: {
        schema: 'notemd.cdr/document-session/v999',
        head: fixture(),
        receipts: [],
        proposals: [],
        assessments: [],
        audit: [],
      },
    }

    await expect(PersistentDocumentSession.open(fixture(), sequentialIds('test'), store))
      .rejects.toBeInstanceOf(InvalidSessionStateError)
  })
})

describe('HostBridgeAggregateStore', () => {
  it('uses the versioned host wire contract without translating durable state', async () => {
    const calls: Array<{ method: string; params: unknown }> = []
    const state: DocumentSessionState = {
      schema: DOCUMENT_SESSION_STATE_SCHEMA,
      head: fixture(),
      revisionHistory: [],
      receipts: [],
      proposals: [],
      assessments: [],
      audit: [],
    }
    const adapter = new HostBridgeAggregateStore(async (method, params) => {
      calls.push({ method, params })
      return method === CDR_REPOSITORY_LOAD_METHOD
        ? { kind: 'loaded', generation: 1, aggregate: state }
        : { kind: 'committed', generation: 2 }
    })

    await expect(adapter.load('document-1')).resolves.toEqual({ generation: 1, aggregate: state })
    await expect(adapter.commit({ documentId: 'document-1', expectedGeneration: 1, aggregate: state }))
      .resolves.toEqual({ kind: 'committed', generation: 2 })
    expect(calls).toEqual([
      { method: CDR_REPOSITORY_LOAD_METHOD, params: { document_id: 'document-1' } },
      {
        method: CDR_REPOSITORY_COMMIT_METHOD,
        params: { document_id: 'document-1', expected_generation: 1, aggregate: state },
      },
    ])
  })

  it('uses the conflict snapshot returned by commit', async () => {
    const state: DocumentSessionState = {
      schema: DOCUMENT_SESSION_STATE_SCHEMA,
      head: fixture(),
      revisionHistory: [],
      receipts: [],
      proposals: [],
      assessments: [],
      audit: [],
    }
    const adapter = new HostBridgeAggregateStore(async () => ({
      kind: 'conflict',
      current: { generation: 7, aggregate: state },
    }))

    await expect(adapter.commit({ documentId: 'document-1', expectedGeneration: 6, aggregate: state }))
      .resolves.toEqual({ kind: 'conflict', current: { generation: 7, aggregate: state } })
  })

  it('rejects impossible generations and extra response fields', async () => {
    await expect(new HostBridgeAggregateStore(async () => ({
      kind: 'loaded', generation: 0, aggregate: {},
    })).load('document-1')).rejects.toBeInstanceOf(RepositoryIOError)

    await expect(new HostBridgeAggregateStore(async () => ({
      kind: 'committed', generation: 3, extra: true,
    })).commit({
      documentId: 'document-1',
      expectedGeneration: 1,
      aggregate: {
        schema: DOCUMENT_SESSION_STATE_SCHEMA,
        head: fixture(), revisionHistory: [], receipts: [], proposals: [], assessments: [], audit: [],
      },
    })).rejects.toBeInstanceOf(RepositoryIOError)
  })
})
