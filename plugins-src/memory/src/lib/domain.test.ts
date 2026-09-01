import { describe, expect, it } from 'vitest'
import {
  approvalForPending,
  approvalKindFor,
  approvalLabels,
  categoryLabel,
  currentClaims,
  hostError,
  pendingClaims,
  subjectLabel,
  temporalLabel,
} from './domain'
import type { EffectiveClaim, MemoryClaimRevision, PendingClaim } from './types'

const claim = (overrides: Partial<MemoryClaimRevision> = {}): MemoryClaimRevision => ({
  schema: 'notemd.memory/claim-revision/v2', claim_id: 'claim-1', revision_id: 'revision-1', parents: [],
  claim_kind: 'preference', subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self' },
  asserted_by: [{ kind: 'owner', id: 'owner-1' }], recorded_by: { kind: 'host', id: 'notemd.memory-ui' },
  recorded_at: '2026-09-01T08:30:00Z', text: '回答先给出结论。', projection: { target: 'user', category: 'preferences', visibility: 'projection' },
  workflow: { state: 'approved' }, lifecycle: { state: 'active' }, temporal: { valid_from: '2026-09-01T08:30:00Z' },
  epistemic: { basis: 'owner-stated', representation_certainty: 'high', truth_status: 'not-assessed', truth_confidence: 'unknown' },
  trust_tier: 'stable-preference', risk_class: 'informational', salience: 'normal', polarity: 'positive', sensitivity: 'normal',
  context: { spaces: ['work/hemory'], applies_when: [], excludes_when: [] },
  consent: { scope: 'personal-assistant-only', allowed_purposes: ['planning', 'writing'], external_provider_policy: 'prompt' },
  agent_use: { guidance: '先给出结论。', avoid_error: '不要扩张为行动授权。' },
  decision: { verdict: 'approve', approval_kind: 'self-representation', actor_id: 'human:bruce', decided_at: '2026-09-01T08:30:00Z' },
  payload_sha256: 'sha-1', ...overrides,
})

describe('Memory Protocol v2 domain', () => {
  it('filters only current approved active claims and keeps pinned negative claims first', () => {
    const normal: EffectiveClaim = { claim: claim(), application_state: 'current' }
    const pinned: EffectiveClaim = { claim: claim({ claim_id: 'claim-2', revision_id: 'revision-2', text: '不要泄露私人内容。', salience: 'pinned', polarity: 'negative' }), application_state: 'current' }
    const stale: EffectiveClaim = { claim: claim({ claim_id: 'claim-3' }), application_state: 'stale' }
    expect(currentClaims([normal, stale, pinned], '', 'all').map((item) => item.claim.claim_id)).toEqual(['claim-2', 'claim-1'])
    expect(currentClaims([normal], '结论', 'user')).toHaveLength(1)
    expect(currentClaims([normal], '结论', 'memory')).toHaveLength(0)
  })

  it('keeps approval semantics distinct by claim kind', () => {
    expect(approvalKindFor('preference')).toBe('self-representation')
    expect(approvalKindFor('boundary')).toBe('behavioral-authorization')
    expect(approvalKindFor('material-fact')).toBe('factual-verification')
    expect(approvalLabels['self-representation'].explanation).toContain('不验证外部事实')
    expect(approvalLabels['behavioral-authorization'].button).toBe('允许此行为')
    expect(approvalLabels['factual-verification'].button).toBe('确认事实正确')
  })

  it('sorts action-sensitive pending first without upgrading its semantics', () => {
    const informational: PendingClaim = { revision: claim({ workflow: { state: 'pending' }, decision: undefined }), expected_sha256: 'a', expected_heads: [] }
    const boundary: PendingClaim = { revision: claim({ claim_id: 'claim-2', revision_id: 'revision-2', claim_kind: 'boundary', workflow: { state: 'pending' }, risk_class: 'action-sensitive', decision: undefined }), expected_sha256: 'b', expected_heads: [] }
    expect(pendingClaims([informational, boundary])[0].revision.claim_id).toBe('claim-2')
    expect(approvalForPending(boundary)).toBe('behavioral-authorization')
  })

  it('renders subject, category and valid time without treating record time as valid time', () => {
    const item = claim({ subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self', label: 'Bruce' } })
    expect(subjectLabel(item)).toBe('Bruce')
    expect(categoryLabel('user', 'preferences')).toBe('偏好')
    expect(temporalLabel(item)).toContain('2026年9月1日')
    expect(temporalLabel(claim({ temporal: {} }))).toBe('未声明有效时间')
  })

  it('turns an unsupported v2 Host into a clear compatibility message', () => {
    expect(hostError(new Error('unknown method host.memory.v2.snapshot'))).toContain('宿主尚未提供 Memory Protocol v2 RPC')
  })
})
