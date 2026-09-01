import type {
  AddClaimInput,
  ClaimMutationInput,
  ContextManifestReceipt,
  ContextPreview,
  ContextRequest,
  MemorySnapshotV2,
  MigrationDryRun,
  PendingDecisionInput,
  ResolveConflictInput,
  Salience,
  WriteReceipt,
} from './types'

export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global { interface Window { notemd: NotemdBridge } }

export function bridge(): NotemdBridge {
  if (!window.notemd) throw new Error('window.notemd bridge missing')
  return window.notemd
}

export function requestId(prefix = 'memory-ui'): string {
  const id = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`
  return `${prefix}/${id}`
}

export async function memorySnapshot(): Promise<MemorySnapshotV2> {
  const value = await bridge().request('host.memory.v2.snapshot', { as_of_valid_time: new Date().toISOString() })
  if (!['v2', 'legacy', 'recovery', 'read-only'].includes(value?.mode)) {
    throw new Error('MEMORY_PROTOCOL_UNSUPPORTED: Host v2 snapshot is missing the plugin view mode')
  }
  return {
    ...value,
    claims: value.claims ?? [],
    pending: value.pending ?? [],
    conflicts: value.conflicts ?? [],
    history: value.history ?? [],
    health: value.health ?? {
      status: value.mode === 'v2' ? 'attention' : 'unsupported',
      message: 'Host 未返回 Memory 健康状态',
      pending_count: value.pending?.length ?? 0,
      conflict_count: value.conflicts?.length ?? 0,
      integrity_errors: [],
    },
  } as MemorySnapshotV2
}

export function memoryAdd(input: AddClaimInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.add', input)
}

export function memoryApprove(input: PendingDecisionInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.approve', input)
}

export function memoryReject(input: PendingDecisionInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.reject', input)
}

export function memoryIgnore(input: PendingDecisionInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.ignore', input)
}

export function memoryDeleteCandidate(input: PendingDecisionInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.delete', input)
}

export function memorySetSalience(input: ClaimMutationInput & { salience: Salience }): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.setSalience', input)
}

export function memoryDelete(input: ClaimMutationInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.delete', { ...input, delete_kind: 'claim-tombstone' })
}

export function memoryResolve(input: ResolveConflictInput): Promise<WriteReceipt> {
  return bridge().request('host.memory.v2.resolve', input)
}

export async function memoryContext(input: ContextRequest): Promise<ContextPreview> {
  const value = await bridge().request('host.memory.v2.context', input)
  if (Array.isArray(value?.selected)) return value as ContextPreview
  return {
    request: value?.request ?? input,
    selected: (value?.claims ?? []).map((claim: any) => ({
      claim_id: claim.claim_id,
      revision_id: claim.revision_id,
      reasons: claim.do_not_rely ? ['selected', 'do-not-rely'] : ['selected'],
      text: claim.text,
    })),
    excluded_summary: value?.excluded_summary ?? {},
    conflicts: (value?.conflicts ?? []).map((conflict: any) => ({ conflict_id: conflict.conflict_id, action_allowed: conflict.action_allowed })),
    redactions: value?.redactions ?? 0,
    policy_result: value?.policy_result ?? { external_action_allowed: value?.action_allowed === true },
  }
}

export async function memoryContextManifest(input: ContextRequest): Promise<ContextManifestReceipt> {
  const value = await bridge().request('host.memory.v2.contextManifest', input)
  return {
    manifest_id: value.manifest_id,
    payload_sha256: value.payload_sha256,
    selected_count: value.selected_count ?? value.selected?.length ?? 0,
  }
}

export function memoryCheck(): Promise<MemorySnapshotV2['health']> {
  return bridge().request('host.memory.v2.check', {})
}

export function memoryMigrationDryRun(): Promise<MigrationDryRun> {
  return bridge().request('host.memory.v2.migrate', { mode: 'dry-run' })
}

export async function toast(level: 'success' | 'info' | 'warn' | 'error', message: string, detail?: string): Promise<void> {
  try { await bridge().request('host.toast', { level, message, detail }) } catch { /* non-critical */ }
}
