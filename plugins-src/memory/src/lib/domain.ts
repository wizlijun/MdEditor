import type { MemoryEntry, Proposal, Scope } from './types'

const priorityRank = { critical: 0, high: 1, normal: 2, low: 3 } as const
const polarityRank = { negative: 0, positive: 1, neutral: 2 } as const

export function filterEntries(entries: MemoryEntry[], query: string, scope: 'all' | Scope, status: string, highOnly: boolean, polarity: 'all' | MemoryEntry['polarity'] = 'all'): MemoryEntry[] {
  const q = query.trim().toLocaleLowerCase()
  return entries.filter((entry) =>
    entry.scope !== 'user-owner'
    && (scope === 'all' || entry.scope === scope)
    && (status === 'all' || entry.status === status)
    && (!highOnly || entry.priority === 'high' || entry.priority === 'critical')
    && (polarity === 'all' || entry.polarity === polarity)
    && (!q || `${entry.text} ${entry.section} ${entry.source ?? ''} ${entry.polarity} ${entry.epistemic_status}`.toLocaleLowerCase().includes(q)),
  ).sort((a, b) => priorityRank[a.priority] - priorityRank[b.priority]
    || polarityRank[a.polarity] - polarityRank[b.polarity]
    || a.text.localeCompare(b.text))
}

export function pendingProposals(proposals: Proposal[]): Proposal[] {
  return proposals.filter((proposal) => proposal.decision === 'pending').sort((a, b) =>
    Number(b.proposal.action_sensitive) - Number(a.proposal.action_sensitive)
    || a.created.localeCompare(b.created))
}

export function describeDelta(proposal: Proposal, entries: MemoryEntry[]): { before: string; after: string } {
  const target = entries.find((entry) => entry.id === proposal.proposal.target_id)
  if (proposal.proposal.operation === 'create') return { before: '—', after: proposal.text }
  if (proposal.proposal.operation === 'revoke') return { before: target?.text ?? '—', after: '撤销（保留历史）' }
  if (proposal.proposal.operation === 'set-priority') return {
    before: target ? `${target.priority}: ${target.text}` : '—',
    after: `${proposal.proposal.suggested_priority ?? 'normal'}: ${target?.text ?? ''}`,
  }
  return { before: target?.text ?? '—', after: proposal.text }
}

export type MetadataDelta = {
  priority: { before: string; after: string }
  polarity: { before: string; after: string }
  epistemicStatus: { before: string; after: string }
  certainty: { before: string; after: string }
  agentGuidance: { before: string; after: string }
  avoidError: { before: string; after: string }
}

export function describeMetadataDelta(proposal: Proposal, entries: MemoryEntry[]): MetadataDelta {
  const target = entries.find((entry) => entry.id === proposal.proposal.target_id)
  const inherited = (value: string | undefined, fallback: string | undefined) => value ?? fallback ?? '—'
  return {
    priority: { before: target?.priority ?? '—', after: inherited(proposal.proposal.suggested_priority, target?.priority) },
    polarity: { before: target?.polarity ?? '—', after: inherited(proposal.proposal.suggested_polarity, target?.polarity) },
    epistemicStatus: { before: target?.epistemic_status ?? '—', after: inherited(proposal.proposal.suggested_epistemic_status, target?.epistemic_status) },
    certainty: { before: target?.certainty ?? '—', after: inherited(proposal.proposal.suggested_certainty, target?.certainty) },
    agentGuidance: { before: target?.agent_guidance ?? '—', after: inherited(proposal.proposal.suggested_agent_guidance, target?.agent_guidance) },
    avoidError: { before: target?.avoid_error ?? '—', after: inherited(proposal.proposal.suggested_avoid_error, target?.avoid_error) },
  }
}

export function usageRule(entry: MemoryEntry): string {
  if (entry.status === 'pending') return '尚未确认：先核验来源，不能作为确定事实。'
  if (entry.status !== 'active') return '历史条目：不要作为当前指令或事实使用。'
  if (entry.polarity === 'negative') return entry.avoid_error || '禁止重复此错误。'
  if (entry.polarity === 'positive') return entry.agent_guidance || '在相关情境优先遵循。'
  return entry.agent_guidance || '仅作为有来源的上下文使用。'
}

export function exactDecisionPrompt(proposal: Proposal, entries: MemoryEntry[], heading: string): string {
  const delta = describeDelta(proposal, entries)
  const mergeSources = proposal.proposal.merge_from.length
    ? `\n\nMerge sources:\n${proposal.proposal.merge_from.join('\n')}`
    : ''
  return `${heading}\n\nProposal: ${proposal.proposal.id}\nSHA-256: ${proposal.sha256}\n\nBefore:\n${delta.before}\n\nAfter:\n${delta.after}${mergeSources}`
}
