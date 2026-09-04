/** Domain-neutral use-case coordinator over the durable session port. */

import {
  actorKey,
  GovernedRevisionChangedError,
  intersectDecisions,
  parseGovernanceDecision,
  type ActorRef,
  type ActorSource,
  type Authorizer,
  type ChangePolicy,
  type GovernanceAction,
  type GovernanceDecision,
  type ProfileDescriptor,
  type ProfileCapabilities,
} from '../../../../../src/lib/cdr/governance'
import { parseOperationBatch, type OperationBatch } from '../../../../../src/lib/cdr/operation'
import type {
  Assessment,
  DocumentRevision,
  Proposal,
  SubmitResult,
} from './session'

export class CdrAuthorizationError extends Error {
  readonly code = 'CDR_AUTHORIZATION_DENIED'

  constructor(action: GovernanceAction) {
    super(`CDR_AUTHORIZATION_DENIED: ${action}`)
    this.name = 'CdrAuthorizationError'
  }
}

export interface DocumentSessionPort {
  snapshot(): DocumentRevision
  proposal(changeSetId: string): Proposal | undefined
  submit(batch: OperationBatch, actorId: string, governedRevisionId: string): Promise<SubmitResult>
  propose(batch: OperationBatch, actorId: string, governedRevisionId: string, rationale?: string): Promise<Proposal>
  decideProposal(
    changeSetId: string,
    decision: 'accept' | 'reject',
    actorId: string,
    governedRevisionId: string,
  ): Promise<SubmitResult | null>
  assess(
    blockId: string,
    actorId: string,
    conclusion: Assessment['conclusion'],
    governedRevisionId: string,
    rationale?: string,
  ): Promise<Assessment>
  assessRevision(
    blockId: string,
    blockRevision: string,
    actorId: string,
    conclusion: Assessment['conclusion'],
    governedRevisionId: string,
    rationale?: string,
  ): Promise<Assessment>
}

export type GovernedSubmitResult = SubmitResult | { kind: 'proposed'; proposal: Proposal }

const MAX_GOVERNANCE_RETRIES = 3

function sameProfile(left: ProfileDescriptor, right: ProfileDescriptor): boolean {
  return left.id === right.id && left.version === right.version
}

function immutableBatch(value: OperationBatch): OperationBatch {
  const parsed = parseOperationBatch(value)
  parsed.operations.forEach((operation) => {
    Object.freeze(operation.target)
    Object.freeze(operation.payload)
    Object.freeze(operation)
  })
  Object.freeze(parsed.operations)
  return Object.freeze(parsed)
}

export class CdrApplicationService {
  constructor(
    private readonly documentId: string,
    documentProfile: ProfileDescriptor,
    private readonly session: DocumentSessionPort,
    private readonly actors: ActorSource,
    private readonly authorizer: Authorizer,
    profile: ProfileCapabilities,
  ) {
    if (!documentId) throw new Error('CDR_DOCUMENT_ID_REQUIRED')
    if (!sameProfile(documentProfile, profile.descriptor)) throw new Error('CDR_PROFILE_MISMATCH')
    const evaluate = profile.policy?.evaluate
    this.profilePolicy = evaluate ? Object.freeze({ evaluate: evaluate.bind(profile.policy) }) : undefined
  }

  private readonly profilePolicy: ChangePolicy | undefined

  async submit(batch: OperationBatch, requestedMode: 'propose' | 'apply' = 'apply'): Promise<GovernedSubmitResult> {
    const governedBatch = immutableBatch(batch)
    if (governedBatch.documentId !== this.documentId) throw new Error('CDR_BATCH_DOCUMENT_MISMATCH')
    for (let attempt = 0; attempt < MAX_GOVERNANCE_RETRIES; attempt += 1) {
      const governed = await this.governChange(governedBatch, requestedMode)
      try {
        if (governed.decision === 'propose') {
          return {
            kind: 'proposed',
            proposal: await this.session.propose(
              governedBatch,
              actorKey(governed.actor),
              governed.snapshot.revisionId,
            ),
          }
        }
        return await this.session.submit(
          governedBatch,
          actorKey(governed.actor),
          governed.snapshot.revisionId,
        )
      } catch (error) {
        if (!(error instanceof GovernedRevisionChangedError)) throw error
      }
    }
    throw new Error('CDR_GOVERNANCE_RETRY_EXHAUSTED')
  }

  async propose(batch: OperationBatch, rationale?: string): Promise<Proposal> {
    const governedBatch = immutableBatch(batch)
    if (governedBatch.documentId !== this.documentId) throw new Error('CDR_BATCH_DOCUMENT_MISMATCH')
    for (let attempt = 0; attempt < MAX_GOVERNANCE_RETRIES; attempt += 1) {
      const governed = await this.governChange(governedBatch, 'propose')
      if (governed.decision !== 'propose') throw new Error('CDR_POLICY_PROPOSE_INVARIANT')
      try {
        return await this.session.propose(
          governedBatch,
          actorKey(governed.actor),
          governed.snapshot.revisionId,
          rationale,
        )
      } catch (error) {
        if (!(error instanceof GovernedRevisionChangedError)) throw error
      }
    }
    throw new Error('CDR_GOVERNANCE_RETRY_EXHAUSTED')
  }

  async decideProposal(changeSetId: string, decision: 'accept' | 'reject'): Promise<SubmitResult | null> {
    for (let attempt = 0; attempt < MAX_GOVERNANCE_RETRIES; attempt += 1) {
      const governed = await this.governDecision(changeSetId, decision)
      if (governed.decision !== 'apply') throw new CdrAuthorizationError('decide')
      try {
        return await this.session.decideProposal(
          changeSetId,
          decision,
          actorKey(governed.actor),
          governed.snapshot.revisionId,
        )
      } catch (error) {
        if (!(error instanceof GovernedRevisionChangedError)) throw error
      }
    }
    throw new Error('CDR_GOVERNANCE_RETRY_EXHAUSTED')
  }

  async assess(blockId: string, conclusion: Assessment['conclusion'], rationale?: string): Promise<Assessment> {
    return this.assessTarget(blockId, conclusion, undefined, rationale)
  }

  async assessRevision(
    blockId: string,
    blockRevision: string,
    conclusion: Assessment['conclusion'],
    rationale?: string,
  ): Promise<Assessment> {
    return this.assessTarget(blockId, conclusion, blockRevision, rationale)
  }

  private async assessTarget(
    blockId: string,
    conclusion: Assessment['conclusion'],
    expectedBlockRevision?: string,
    rationale?: string,
  ): Promise<Assessment> {
    for (let attempt = 0; attempt < MAX_GOVERNANCE_RETRIES; attempt += 1) {
      const governed = await this.governAssessment(blockId, conclusion, expectedBlockRevision)
      if (governed.decision !== 'apply') throw new CdrAuthorizationError('assess')
      try {
        return expectedBlockRevision === undefined
          ? await this.session.assess(
              blockId,
              actorKey(governed.actor),
              conclusion,
              governed.snapshot.revisionId,
              rationale,
            )
          : await this.session.assessRevision(
              blockId,
              expectedBlockRevision,
              actorKey(governed.actor),
              conclusion,
              governed.snapshot.revisionId,
              rationale,
            )
      } catch (error) {
        if (!(error instanceof GovernedRevisionChangedError)) throw error
      }
    }
    throw new Error('CDR_GOVERNANCE_RETRY_EXHAUSTED')
  }

  private async authorize(action: GovernanceAction): Promise<{ actor: ActorRef; authorization: GovernanceDecision }> {
    const actor = this.actors.currentActor()
    actorKey(actor)
    const authorization = parseGovernanceDecision(await this.authorizer.authorize({
      documentId: this.documentId,
      actor,
      action,
    }))
    if (authorization === 'deny') throw new CdrAuthorizationError(action)
    return { actor, authorization }
  }

  private policy(action: GovernanceAction) {
    const policy = this.profilePolicy
    if (!policy) throw new CdrAuthorizationError(action)
    return policy
  }

  private snapshot(): DocumentRevision {
    const snapshot = this.session.snapshot()
    if (snapshot.documentId !== this.documentId) throw new Error('CDR_SESSION_DOCUMENT_MISMATCH')
    return snapshot
  }

  private async governChange(
    batch: OperationBatch,
    requestedMode: 'propose' | 'apply',
  ): Promise<{ actor: ActorRef; decision: GovernanceDecision; snapshot: DocumentRevision }> {
    const policy = this.policy('change')
    const { actor, authorization } = await this.authorize('change')
    const snapshot = this.snapshot()
    const decision = parseGovernanceDecision(policy.evaluate({
      documentId: this.documentId,
      actor,
      action: 'change',
      requestedMode,
      snapshot,
      batch,
    }))
    const effective = intersectDecisions(authorization, decision, requestedMode)
    if (effective === 'deny') throw new CdrAuthorizationError('change')
    return { actor, decision: effective, snapshot }
  }

  private async governDecision(
    changeSetId: string,
    decision: 'accept' | 'reject',
  ): Promise<{ actor: ActorRef; decision: GovernanceDecision; snapshot: DocumentRevision }> {
    const policy = this.policy('decide')
    const { actor, authorization } = await this.authorize('decide')
    const snapshot = this.snapshot()
    const changeSet = this.session.proposal(changeSetId)
    if (!changeSet) throw new Error('CDR_PROPOSAL_NOT_FOUND')
    const policyDecision = parseGovernanceDecision(policy.evaluate({
      documentId: this.documentId,
      actor,
      action: 'decide',
      snapshot,
      changeSet,
      decision,
    }))
    return { actor, decision: intersectDecisions(authorization, policyDecision), snapshot }
  }

  private async governAssessment(
    blockId: string,
    conclusion: Assessment['conclusion'],
    expectedBlockRevision?: string,
  ): Promise<{ actor: ActorRef; decision: GovernanceDecision; snapshot: DocumentRevision }> {
    const policy = this.policy('assess')
    const { actor, authorization } = await this.authorize('assess')
    const snapshot = this.snapshot()
    const blockRevision = expectedBlockRevision ?? snapshot.blocks.find((item) => item.blockId === blockId)?.blockRevision
    if (!blockRevision) throw new Error('CDR_BLOCK_NOT_FOUND')
    const policyDecision = parseGovernanceDecision(policy.evaluate({
      documentId: this.documentId,
      actor,
      action: 'assess',
      snapshot,
      target: { blockId, blockRevision },
      conclusion,
    }))
    return { actor, decision: intersectDecisions(authorization, policyDecision), snapshot }
  }
}

export function fixedActorSource(actor: ActorRef): ActorSource {
  const bound = { ...actor }
  actorKey(bound)
  return { currentActor: () => ({ ...bound }) }
}
