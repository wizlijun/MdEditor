import type {
  Authorizer,
  ChangePolicyRequest,
  GovernanceDecision,
  ProfileCapabilities,
} from '../../../../../src/lib/cdr/governance'

export const MEMORY_SELF_PROFILE = 'notemd.memory.self' as const
export const MEMORY_SELF_PROFILE_DESCRIPTOR = Object.freeze({ id: MEMORY_SELF_PROFILE, version: 1 })

function memorySelfDecision(request: ChangePolicyRequest): GovernanceDecision {
  if (request.action === 'decide') return request.actor.kind === 'human' ? 'apply' : 'deny'
  if (request.action === 'assess') return request.actor.kind === 'human' || request.actor.kind === 'agent'
    ? 'apply'
    : 'deny'
  if (request.actor.kind === 'agent') return 'propose'
  return request.actor.kind === 'human' ? 'apply' : 'deny'
}

export const memorySelfProfile: ProfileCapabilities = {
  descriptor: MEMORY_SELF_PROFILE_DESCRIPTOR,
  policy: { evaluate: memorySelfDecision },
}

/**
 * Stage 1A local authorization adapter. Actor values are bound while creating
 * the application service; callers of individual use cases cannot supply one.
 * A future Host identity adapter replaces this object without changing Core.
 */
export const localMemoryAuthorizer: Authorizer = {
  async authorize({ actor }) {
    return actor.kind === 'human' || actor.kind === 'agent' ? 'apply' : 'deny'
  },
}
