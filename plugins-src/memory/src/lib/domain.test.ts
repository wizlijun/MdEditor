import { describe, expect, it } from 'vitest'
import {
  approvalForPending,
  approvalKindFor,
  approvalLabels,
  categoryLabel,
  currentClaims,
  groupCurrentClaims,
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

  it('groups confirmed claims by projection and category in projected-file order', () => {
    const userNormal: EffectiveClaim = { claim: claim({ claim_id: 'user-b', revision_id: 'user-b', text: '普通偏好。' }), application_state: 'current' }
    const userPinned: EffectiveClaim = { claim: claim({ claim_id: 'user-a', revision_id: 'user-a', text: '重点偏好。', salience: 'pinned' }), application_state: 'current' }
    const memoryDecision: EffectiveClaim = { claim: claim({ claim_id: 'memory-a', revision_id: 'memory-a', projection: { target: 'memory', category: 'decisions', visibility: 'projection' }, claim_kind: 'decision' }), application_state: 'current' }
    const structured: EffectiveClaim = { claim: claim({ claim_id: 'structured-a', revision_id: 'structured-a', projection: { target: 'user', category: 'boundaries', visibility: 'trusted-agent' } }), application_state: 'current' }

    const sorted = currentClaims([userNormal, structured, userPinned, memoryDecision])
    const groups = groupCurrentClaims(sorted)
    expect(groups.map((group) => group.label)).toEqual(['MEMORY.md', 'USER.md', '仅结构化上下文'])
    expect(groups[1].categories[0].label).toBe('偏好')
    expect(groups[1].categories[0].items.map((item) => item.claim.claim_id)).toEqual(['user-a', 'user-b'])
    expect(groups[2].categories[0].label).toBe('USER · 边界')
    expect(currentClaims([structured], '', 'user')).toHaveLength(0)
    expect(currentClaims([structured], '', 'structured')).toHaveLength(1)
  })

  it('offers recent and oldest ordering within each projection category', () => {
    const older: EffectiveClaim = { claim: claim({ claim_id: 'older', recorded_at: '2026-08-01T00:00:00Z' }), application_state: 'current' }
    const newer: EffectiveClaim = { claim: claim({ claim_id: 'newer', revision_id: 'newer', recorded_at: '2026-09-01T00:00:00Z' }), application_state: 'current' }
    const invalid: EffectiveClaim = { claim: claim({ claim_id: 'invalid', revision_id: 'invalid', recorded_at: 'not-a-date' }), application_state: 'current' }
    expect(currentClaims([older, invalid, newer], '', 'all', 'recent').map((item) => item.claim.claim_id)).toEqual(['newer', 'older', 'invalid'])
    expect(currentClaims([newer, older], '', 'all', 'oldest').map((item) => item.claim.claim_id)).toEqual(['older', 'newer'])
  })

  it('sorts mixed text deterministically with numeric collation', () => {
    const ten: EffectiveClaim = { claim: claim({ claim_id: 'ten', text: '主题 10' }), application_state: 'current' }
    const two: EffectiveClaim = { claim: claim({ claim_id: 'two', revision_id: 'two', text: '主题 2' }), application_state: 'current' }
    expect(currentClaims([ten, two], '', 'all', 'text').map((item) => item.claim.claim_id)).toEqual(['two', 'ten'])
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
