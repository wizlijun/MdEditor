import { describe, expect, it } from 'vitest'
import { describeDelta, describeMetadataDelta, exactDecisionPrompt, filterEntries, pendingProposals, usageRule } from './domain'
import type { MemoryEntry, Proposal } from './types'

const entry: MemoryEntry = {
  id: 'e1', scope: 'memory', section: 'S', text: 'Keep evidence precise', revision: 1,
  status: 'active', priority: 'high', polarity: 'negative', epistemic_status: 'owner-stated', certainty: 'high',
  agent_guidance: 'Respect the boundary.', avoid_error: 'Never disclose it.', classification_complete: true,
  document: 'MEMORY.md', legacy: false,
}

const proposal = (operation: Proposal['proposal']['operation'], decision: Proposal['decision'] = 'pending'): Proposal => ({
  type: 'Memory Proposal', title: 'x', created: '2026-09-01T00:00:00Z',
  proposal: { version: 1, id: 'p1', scope: 'memory', operation, target_id: 'e1', base_revision: 1,
    dedupe_key: 'k', action_sensitive: false, merge_from: [], suggested_priority: 'normal' },
  generated: { by: 'agent/x', at: '2026-09-01T00:00:00Z' }, sources: [], text: 'Updated fact', reason: '',
  path: 'inbox/memory-candidates/p.md', sha256: 'a', decision,
})

describe('memory domain', () => {
  it('filters by search and high priority', () => {
    expect(filterEntries([entry], 'evidence', 'all', 'active', true)).toEqual([entry])
    expect(filterEntries([entry], 'missing', 'all', 'active', true)).toEqual([])
    expect(filterEntries([{ ...entry, priority: 'critical' }], '', 'all', 'active', true, 'negative')).toHaveLength(1)
    expect(filterEntries([entry], '', 'all', 'active', false, 'positive')).toEqual([])
    expect(filterEntries([{ ...entry, scope: 'user-owner' }], '', 'all', 'active', false)).toEqual([])
  })
  it('keeps only pending proposals', () => {
    expect(pendingProposals([proposal('replace'), proposal('replace', 'approved')])).toHaveLength(1)
  })
  it('renders a revoke as a delta instead of physical deletion', () => {
    expect(describeDelta(proposal('revoke'), [entry])).toEqual({ before: entry.text, after: '撤销（保留历史）' })
  })
  it('binds approval to the exact id, hash and displayed delta', () => {
    const prompt = exactDecisionPrompt(proposal('replace'), [entry], 'Confirm exact change')
    expect(prompt).toContain('Proposal: p1')
    expect(prompt).toContain('SHA-256: a')
    expect(prompt).toContain(`Before:\n${entry.text}`)
    expect(prompt).toContain('After:\nUpdated fact')
  })
  it('shows the complete before and after behavior metadata', () => {
    const change = { ...proposal('replace'), proposal: { ...proposal('replace').proposal,
      suggested_agent_guidance: 'Apply only with consent.', suggested_avoid_error: 'Never infer consent.' } }
    const metadata = describeMetadataDelta(change, [entry])
    expect(metadata.agentGuidance).toEqual({ before: 'Respect the boundary.', after: 'Apply only with consent.' })
    expect(metadata.avoidError).toEqual({ before: 'Never disclose it.', after: 'Never infer consent.' })
  })
  it('makes negative guidance explicit and sorts it before neutral context', () => {
    const neutral = { ...entry, id: 'e2', priority: 'high' as const, polarity: 'neutral' as const }
    expect(filterEntries([neutral, entry], '', 'all', 'active', false).map((item) => item.id)).toEqual(['e1', 'e2'])
    expect(usageRule(entry)).toBe('Never disclose it.')
    expect(usageRule({ ...entry, status: 'pending' })).toContain('不能作为确定事实')
  })
})
