import type { MemoryEntry, Proposal, Scope } from './types'

export function filterEntries(entries: MemoryEntry[], query: string, scope: 'all' | Scope, status: string, highOnly: boolean): MemoryEntry[] {
  const q = query.trim().toLocaleLowerCase()
  return entries.filter((entry) =>
    (scope === 'all' || entry.scope === scope)
    && (status === 'all' || entry.status === status)
    && (!highOnly || entry.priority === 'high')
    && (!q || `${entry.text} ${entry.section} ${entry.source ?? ''}`.toLocaleLowerCase().includes(q)),
  )
}

export function pendingProposals(proposals: Proposal[]): Proposal[] {
  return proposals.filter((proposal) => proposal.decision === 'pending')
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

export function exactDecisionPrompt(proposal: Proposal, entries: MemoryEntry[], heading: string): string {
  const delta = describeDelta(proposal, entries)
  const mergeSources = proposal.proposal.merge_from.length
    ? `\n\nMerge sources:\n${proposal.proposal.merge_from.join('\n')}`
    : ''
  return `${heading}\n\nProposal: ${proposal.proposal.id}\nSHA-256: ${proposal.sha256}\n\nBefore:\n${delta.before}\n\nAfter:\n${delta.after}${mergeSources}`
}
