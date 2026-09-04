import type { DocumentRevision } from './core'
import type { OperationBatch } from './operation'

export type ActorKind = 'human' | 'agent' | 'service'

export interface ActorRef {
  kind: ActorKind
  id: string
}

export interface ActorSource {
  currentActor(): ActorRef
}

export type GovernanceDecision = 'deny' | 'propose' | 'apply'
export type GovernanceAction = 'change' | 'decide' | 'assess'

export interface AuthorizationRequest {
  documentId: string
  actor: ActorRef
  action: GovernanceAction
}

export interface Authorizer {
  authorize(request: AuthorizationRequest): Promise<GovernanceDecision>
}

interface PolicyRequestBase extends AuthorizationRequest {
  snapshot: DocumentRevision
}

export interface PolicyChangeSetView {
  changeSetId: string
  actorId: string
  status: 'pending' | 'applied' | 'conflicted' | 'rejected'
  batch: OperationBatch
}

export type ChangePolicyRequest =
  | PolicyRequestBase & {
      action: 'change'
      requestedMode: 'propose' | 'apply'
      batch: OperationBatch
    }
  | PolicyRequestBase & {
      action: 'decide'
      changeSet: PolicyChangeSetView
      decision: 'accept' | 'reject'
    }
  | PolicyRequestBase & {
      action: 'assess'
      target: { blockId: string; blockRevision: string }
      conclusion: 'verified' | 'needs-review'
    }

export interface ChangePolicy {
  evaluate(request: ChangePolicyRequest): GovernanceDecision
}

export interface ProfileDescriptor {
  readonly id: string
  readonly version: number
}

export interface ProfileCapabilities {
  readonly descriptor: ProfileDescriptor
  readonly policy?: ChangePolicy
}

/** Retry signal required by the governed session port, independent of its repository implementation. */
export class GovernedRevisionChangedError extends Error {
  readonly code = 'CDR_GOVERNED_REVISION_CHANGED'

  constructor() {
    super('CDR_GOVERNED_REVISION_CHANGED')
    this.name = 'GovernedRevisionChangedError'
  }
}

const decisionRank: Record<GovernanceDecision, number> = { deny: 0, propose: 1, apply: 2 }

export function parseGovernanceDecision(value: unknown): GovernanceDecision {
  if (value !== 'deny' && value !== 'propose' && value !== 'apply') {
    throw new Error('CDR_GOVERNANCE_DECISION_INVALID')
  }
  return value
}

/** Profile policy can only preserve or reduce the Host authorization. */
export function intersectDecisions(
  authorization: GovernanceDecision,
  policy: GovernanceDecision,
  requestedMode?: 'propose' | 'apply',
): GovernanceDecision {
  parseGovernanceDecision(authorization)
  parseGovernanceDecision(policy)
  const requested: GovernanceDecision = requestedMode === 'propose' ? 'propose' : 'apply'
  const decisions: GovernanceDecision[] = [authorization, policy, requested]
  return decisions.reduce<GovernanceDecision>(
    (lowest, value) => decisionRank[value] < decisionRank[lowest] ? value : lowest,
    'apply',
  )
}

export function actorKey(actor: ActorRef): string {
  if (actor.kind !== 'human' && actor.kind !== 'agent' && actor.kind !== 'service') {
    throw new Error('CDR_ACTOR_INVALID')
  }
  if (!actor.id || !/^[a-z0-9][a-z0-9._:/-]*$/i.test(actor.id)) throw new Error('CDR_ACTOR_INVALID')
  return `${actor.kind}:${actor.id}`
}
