export type Scope = 'user-owner' | 'user-profile' | 'memory'
export type Operation = 'create' | 'replace' | 'merge' | 'revoke' | 'set-priority'
export type Priority = 'normal' | 'high'
export type Decision = 'pending' | 'approved' | 'rejected' | 'conflict'

export interface MemoryEntry {
  id: string
  scope: Scope
  section: string
  text: string
  revision: number
  status: string
  priority: Priority
  proposal?: string
  approved_by?: string
  approved_at?: string
  source?: string
  document: string
  legacy: boolean
}

export interface Proposal {
  type: 'Memory Proposal'
  title: string
  created: string
  proposal: {
    version: number
    id: string
    scope: Scope
    operation: Operation
    target_id?: string
    base_revision?: number
    section?: string
    suggested_priority?: Priority
    dedupe_key: string
    action_sensitive: boolean
    merge_from: string[]
  }
  generated: { by: string; at: string }
  sources: { id: string; resource: string; title?: string }[]
  text: string
  reason: string
  path: string
  sha256: string
  decision: Decision
}

export interface Snapshot {
  entries: MemoryEntry[]
  proposals: Proposal[]
  integrity: { managed: boolean; drift: boolean; errors: string[] }
  owner_actor?: string
}

export interface ProposeInput {
  scope: Scope
  operation: Operation
  text: string
  source: string
  by: string
  dedupe_key: string
  reason: string
  target_id?: string
  base_revision?: number
  section?: string
  priority?: Priority
  merge_from: string[]
}

export interface DecideInput {
  proposal_id: string
  expected_sha256: string
  action: 'approve' | 'reject'
  actor: string
  human_confirmed: boolean
  reason?: string
}
