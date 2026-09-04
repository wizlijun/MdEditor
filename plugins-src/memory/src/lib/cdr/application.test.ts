import { describe, expect, it, vi } from 'vitest'
import type {
  Authorizer,
  ChangePolicyRequest,
  GovernanceDecision,
  ProfileCapabilities,
} from '../../../../../src/lib/cdr/governance'
import { GovernedRevisionChangedError } from '../../../../../src/lib/cdr/governance'
import { CdrApplicationService, CdrAuthorizationError, fixedActorSource, type DocumentSessionPort } from './application'
import { MEMORY_SELF_PROFILE_DESCRIPTOR, memorySelfProfile } from './profile'
import {
  InMemoryDocumentSession,
  sequentialIds,
  type DocumentRevision,
  type OperationBatch,
  type ReplaceBlockOperation,
  type SubmitResult,
} from './session'

function fixture(): DocumentRevision {
  return {
    documentId: 'document-1',
    revisionId: 'revision-1',
    blocks: [{ blockId: 'block-1', blockRevision: 'block-1/1', markdown: 'Before.' }],
  }
}

function batch(requestId = 'request-1'): OperationBatch & { operations: readonly [ReplaceBlockOperation] } {
  return {
    requestId,
    documentId: 'document-1',
    baseRevisionId: 'revision-1',
    operations: [{
      kind: 'block.replace',
      operationId: `${requestId}/operation`,
      target: { blockId: 'block-1', expectedBlockRevision: 'block-1/1' },
      payload: { content: 'After.' },
    }],
  }
}

const allow: Authorizer = { authorize: async () => 'apply' }

describe('CdrApplicationService', () => {
  it('applies a human change while downgrading an agent apply request to a proposal', async () => {
    const humanSession = new InMemoryDocumentSession(fixture(), sequentialIds('human'))
    const human = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, humanSession,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, memorySelfProfile,
    )
    const applied = await human.submit(batch())
    expect(applied.kind).toBe('applied')
    expect(humanSession.audit()[0].actorId).toBe('human:local')

    const agentSession = new InMemoryDocumentSession(fixture(), sequentialIds('agent'))
    const agent = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, agentSession,
      fixedActorSource({ kind: 'agent', id: 'organizer' }), allow, memorySelfProfile,
    )
    const proposed = await agent.submit(batch(), 'apply')
    expect(proposed.kind).toBe('proposed')
    expect(agentSession.snapshot()).toEqual(fixture())
    expect(agentSession.proposals()[0].actorId).toBe('agent:organizer')
  })

  it('denies before reading or mutating the session and exposes no actor parameter on use cases', async () => {
    const calls: string[] = []
    const deniedSession = {
      snapshot: vi.fn(() => { calls.push('snapshot'); return fixture() }),
      proposal: vi.fn(),
      submit: vi.fn(),
      propose: vi.fn(),
      decideProposal: vi.fn(),
      assess: vi.fn(),
    } as unknown as DocumentSessionPort
    const deny: Authorizer = { authorize: async () => { calls.push('authorize'); return 'deny' } }
    const app = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, deniedSession,
      fixedActorSource({ kind: 'agent', id: 'blocked' }), deny, memorySelfProfile,
    )

    await expect(app.submit(batch())).rejects.toBeInstanceOf(CdrAuthorizationError)
    expect(calls).toEqual(['authorize'])
    expect(deniedSession.submit).not.toHaveBeenCalled()
    expect(deniedSession.propose).not.toHaveBeenCalled()
  })

  it('rejects malformed authorization and a missing policy before reading the session', async () => {
    const session = {
      snapshot: vi.fn(() => fixture()),
      proposal: vi.fn(),
      submit: vi.fn(),
      propose: vi.fn(),
      decideProposal: vi.fn(),
      assess: vi.fn(),
    } as unknown as DocumentSessionPort
    const malformed: Authorizer = {
      authorize: async () => 'unexpected' as GovernanceDecision,
    }
    const actor = fixedActorSource({ kind: 'human', id: 'local' })
    const malformedApp = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, session, actor, malformed, memorySelfProfile,
    )
    await expect(malformedApp.submit(batch())).rejects.toThrow('CDR_GOVERNANCE_DECISION_INVALID')
    expect(session.snapshot).not.toHaveBeenCalled()

    const authorize = vi.fn(async () => 'apply' as const)
    const noPolicy: ProfileCapabilities = { descriptor: { id: 'example.no-policy', version: 1 } }
    const noPolicyApp = new CdrApplicationService(
      'document-1', noPolicy.descriptor, session, actor, { authorize }, noPolicy,
    )
    await expect(noPolicyApp.submit(batch())).rejects.toBeInstanceOf(CdrAuthorizationError)
    expect(authorize).not.toHaveBeenCalled()
    expect(session.snapshot).not.toHaveBeenCalled()
  })

  it('parses and freezes one detached batch before authorization', async () => {
    let releaseAuthorization: ((decision: GovernanceDecision) => void) | undefined
    const authorize = vi.fn(() => new Promise<GovernanceDecision>((resolve) => {
      releaseAuthorization = resolve
    }))
    const observed: OperationBatch[] = []
    const profile: ProfileCapabilities = {
      descriptor: { id: 'example.immutable-input', version: 1 },
      policy: {
        evaluate(request) {
          if (request.action === 'change') observed.push(request.batch)
          return 'apply'
        },
      },
    }
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('immutable-input'))
    const app = new CdrApplicationService(
      'document-1', profile.descriptor, session,
      fixedActorSource({ kind: 'human', id: 'local' }), { authorize }, profile,
    )
    const callerOwned = batch('immutable-request')

    const pending = app.submit(callerOwned)
    ;(callerOwned.operations[0].payload as { content: string }).content = 'Mutated after authorization started.'
    releaseAuthorization?.('apply')
    await expect(pending).resolves.toMatchObject({ kind: 'applied' })

    expect(observed[0].operations[0]).toMatchObject({ payload: { content: 'After.' } })
    expect(Object.isFrozen(observed[0])).toBe(true)
    expect(Object.isFrozen(observed[0].operations)).toBe(true)
    expect(Object.isFrozen(observed[0].operations[0])).toBe(true)
    expect(session.snapshot().blocks[0].markdown).toBe('After.')
  })

  it('rejects a malformed batch before authorization or session reads', async () => {
    const session = {
      snapshot: vi.fn(() => fixture()),
      proposal: vi.fn(),
      submit: vi.fn(),
      propose: vi.fn(),
      decideProposal: vi.fn(),
      assess: vi.fn(),
    } as unknown as DocumentSessionPort
    const authorize = vi.fn(async () => 'apply' as const)
    const app = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, session,
      fixedActorSource({ kind: 'human', id: 'local' }), { authorize }, memorySelfProfile,
    )

    await expect(app.submit({ ...batch(), extra: true } as unknown as OperationBatch))
      .rejects.toThrow('CDR_OPERATION_INVALID')
    expect(authorize).not.toHaveBeenCalled()
    expect(session.snapshot).not.toHaveBeenCalled()
  })

  it('rejects a document whose persisted profile does not match the runtime profile', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('mismatch'))
    expect(() => new CdrApplicationService(
      'document-1', { id: 'another.profile', version: 1 }, session,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, memorySelfProfile,
    )).toThrow('CDR_PROFILE_MISMATCH')
  })

  it('accepts a self proposal in one session transition and rejects stale acceptance', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('decision'))
    const agent = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, session,
      fixedActorSource({ kind: 'agent', id: 'organizer' }), allow, memorySelfProfile,
    )
    const human = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, session,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, memorySelfProfile,
    )
    const proposal = await agent.propose(batch('proposal'))
    const accepted = await human.decideProposal(proposal.changeSetId, 'accept')
    expect(accepted?.kind).toBe('applied')
    expect(session.proposals()[0].status).toBe('applied')
    expect(session.snapshot().blocks[0].markdown).toBe('After.')

    const staleSession = new InMemoryDocumentSession(fixture(), sequentialIds('stale'))
    const staleAgent = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, staleSession,
      fixedActorSource({ kind: 'agent', id: 'organizer' }), allow, memorySelfProfile,
    )
    const staleHuman = new CdrApplicationService(
      'document-1', MEMORY_SELF_PROFILE_DESCRIPTOR, staleSession,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, memorySelfProfile,
    )
    const stale = await staleAgent.propose(batch('stale-proposal'))
    await staleHuman.submit({
      ...batch('human-change'),
      operations: [{
        ...batch().operations[0],
        operationId: 'human-change/op',
        payload: { content: 'Human.' },
      }],
    })
    expect((await staleHuman.decideProposal(stale.changeSetId, 'accept'))?.kind).toBe('conflicted')
    expect(staleSession.snapshot().blocks[0].markdown).toBe('Human.')
  })

  it('keeps the application generic when another profile replaces the policy', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('readonly'))
    const readOnlyProfile: ProfileCapabilities = {
      descriptor: { id: 'example.read-only', version: 1 },
      policy: { evaluate: () => 'deny' },
    }
    const app = new CdrApplicationService(
      'document-1', readOnlyProfile.descriptor, session,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, readOnlyProfile,
    )
    await expect(app.submit(batch())).rejects.toBeInstanceOf(CdrAuthorizationError)
    expect(session.snapshot()).toEqual(fixture())
  })

  it('gives policy the exact proposal decision and assessment target', async () => {
    const requests: ChangePolicyRequest[] = []
    const profile: ProfileCapabilities = {
      descriptor: { id: 'example.inspecting', version: 1 },
      policy: {
        evaluate(request) {
          requests.push(request)
          return request.action === 'change' && request.actor.kind === 'agent' ? 'propose' : 'apply'
        },
      },
    }
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('policy-input'))
    const agent = new CdrApplicationService(
      'document-1', profile.descriptor, session,
      fixedActorSource({ kind: 'agent', id: 'organizer' }), allow, profile,
    )
    const human = new CdrApplicationService(
      'document-1', profile.descriptor, session,
      fixedActorSource({ kind: 'human', id: 'local' }), allow, profile,
    )

    const proposal = await agent.propose(batch('policy-proposal'))
    await human.decideProposal(proposal.changeSetId, 'reject')
    await human.assess('block-1', 'needs-review')

    const decideRequest = requests.find((request) => request.action === 'decide')
    expect(decideRequest).toMatchObject({
      action: 'decide',
      decision: 'reject',
      changeSet: {
        changeSetId: proposal.changeSetId,
        actorId: 'agent:organizer',
        status: 'pending',
        batch: batch('policy-proposal'),
      },
    })
    const assessRequest = requests.find((request) => request.action === 'assess')
    expect(assessRequest).toMatchObject({
      action: 'assess',
      target: { blockId: 'block-1', blockRevision: 'block-1/1' },
      conclusion: 'needs-review',
    })
  })

  it('re-authorizes and re-evaluates policy when the governed head changes in the session queue', async () => {
    const snapshots = [fixture(), { ...fixture(), revisionId: 'revision-2' }]
    let current = snapshots[0]
    const guardedRevisions: string[] = []
    const conflicted: SubmitResult = {
      kind: 'conflicted',
      conflict: { code: 'stale-base', message: 'stale' },
      snapshot: snapshots[1],
    }
    const session: DocumentSessionPort = {
      snapshot: vi.fn(() => current),
      proposal: vi.fn(),
      submit: vi.fn(async (_batch, _actorId, governedRevisionId) => {
        guardedRevisions.push(governedRevisionId ?? '')
        if (guardedRevisions.length === 1) {
          current = snapshots[1]
          throw new GovernedRevisionChangedError()
        }
        return conflicted
      }),
      propose: vi.fn(),
      decideProposal: vi.fn(),
      assess: vi.fn(),
    }
    const authorizedRevisions: string[] = []
    const authorize = vi.fn(async () => 'apply' as const)
    const profile: ProfileCapabilities = {
      descriptor: { id: 'example.retry', version: 1 },
      policy: {
        evaluate(request) {
          authorizedRevisions.push(request.snapshot.revisionId)
          return 'apply'
        },
      },
    }
    const app = new CdrApplicationService(
      'document-1', profile.descriptor, session,
      fixedActorSource({ kind: 'human', id: 'local' }), { authorize }, profile,
    )

    await expect(app.submit(batch())).resolves.toEqual(conflicted)
    expect(authorize).toHaveBeenCalledTimes(2)
    expect(authorizedRevisions).toEqual(['revision-1', 'revision-2'])
    expect(guardedRevisions).toEqual(['revision-1', 'revision-2'])
  })

  it('re-governs the later of two concurrent changes on a real session', async () => {
    const initialAuthorizations: Array<(decision: GovernanceDecision) => void> = []
    let authorizationCalls = 0
    const authorize = vi.fn(() => {
      authorizationCalls += 1
      if (authorizationCalls <= 2) {
        return new Promise<GovernanceDecision>((resolve) => initialAuthorizations.push(resolve))
      }
      return Promise.resolve<GovernanceDecision>('apply')
    })
    const governedRevisions: string[] = []
    const profile: ProfileCapabilities = {
      descriptor: { id: 'example.real-queue', version: 1 },
      policy: {
        evaluate(request) {
          governedRevisions.push(request.snapshot.revisionId)
          return 'apply'
        },
      },
    }
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('real-queue'))
    const app = new CdrApplicationService(
      'document-1', profile.descriptor, session,
      fixedActorSource({ kind: 'human', id: 'local' }), { authorize }, profile,
    )
    const firstBatch = batch('concurrent-first')
    const secondBatch = {
      ...batch('concurrent-second'),
      operations: [{ ...batch('concurrent-second').operations[0], payload: { content: 'Second.' } }],
    }

    const firstPromise = app.submit(firstBatch)
    const secondPromise = app.submit(secondBatch)
    expect(initialAuthorizations).toHaveLength(2)
    initialAuthorizations.forEach((resolve) => resolve('apply'))
    const [first, second] = await Promise.all([firstPromise, secondPromise])

    expect(first.kind).toBe('applied')
    expect(second.kind).toBe('conflicted')
    if (first.kind !== 'applied') throw new Error('expected first change to apply')
    expect(governedRevisions).toEqual(['revision-1', 'revision-1', first.snapshot.revisionId])
    expect(authorize).toHaveBeenCalledTimes(3)
    expect(session.snapshot().blocks[0].markdown).toBe('After.')
  })
})
