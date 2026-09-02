// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import { MEMORY_INFERENCE_STATE } from './lib/inference-task'
import type { MemoryClaimRevision, MemorySnapshotV2, PendingClaim, WriteReceipt } from './lib/types'

let component: ReturnType<typeof mount> | null = null
afterEach(() => { if (component) unmount(component); component = null; document.body.innerHTML = ''; vi.useRealTimers() })

const revision = (overrides: Partial<MemoryClaimRevision> = {}): MemoryClaimRevision => ({
  schema: 'notemd.memory/claim-revision/v2', claim_id: 'claim-1', revision_id: 'revision-1', parents: [],
  claim_kind: 'preference', subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self', label: 'Bruce' },
  asserted_by: [{ kind: 'owner', id: 'owner-1' }], recorded_by: { kind: 'host', id: 'notemd.memory-ui' },
  recorded_at: '2026-09-01T08:30:00Z', text: '回答先给出结论。\n不确定内容应明确标注。',
  projection: { target: 'user', category: 'preferences', visibility: 'projection' }, workflow: { state: 'approved' }, lifecycle: { state: 'active' },
  temporal: { valid_from: '2026-09-01T08:30:00Z' },
  epistemic: { basis: 'owner-stated', representation_certainty: 'high', truth_status: 'not-assessed', truth_confidence: 'unknown' },
  trust_tier: 'stable-preference', risk_class: 'informational', salience: 'normal', polarity: 'positive', sensitivity: 'normal',
  context: { spaces: ['work/hemory'], applies_when: [], excludes_when: [] },
  consent: { scope: 'personal-assistant-only', allowed_purposes: ['planning', 'writing'], external_provider_policy: 'prompt' },
  agent_use: { guidance: '先给结论。', avoid_error: '不要扩张为外部行动授权。' },
  decision: { verdict: 'approve', approval_kind: 'self-representation', authority_scope: 'personal-assistant', actor_id: 'human:bruce', decided_at: '2026-09-01T08:30:00Z' },
  evidence: [{ relation: 'evidence-of-speech', resource: '/notes/source.md' }], payload_sha256: 'sha-revision-1', ...overrides,
})

const pending = (overrides: Partial<MemoryClaimRevision> = {}): PendingClaim => ({
  revision: revision({ revision_id: 'pending-1', workflow: { state: 'pending' }, decision: undefined, recorded_by: { kind: 'agent', id: 'agent:daily-summary' }, ...overrides }),
  expected_sha256: overrides.payload_sha256 ?? 'sha-pending-1', expected_heads: [], base_text: '旧版本', source_summary: '/daily/2026-09-01.md',
})

const baseSnapshot = (): MemorySnapshotV2 => ({
  mode: 'v2', protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' },
  owner: { actor_id: 'human:bruce', subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self', label: 'Bruce' } },
  claims: [{ claim: revision(), application_state: 'current' }], pending: [pending()], conflicts: [],
  history: [{ id: 'history-1', claim_id: 'claim-1', revision_id: 'revision-1', operation: 'create-approved', workflow_state: 'approved', lifecycle_state: 'active', actor_id: 'human:bruce', approval_kind: 'self-representation', recorded_at: '2026-09-01T08:30:00Z', summary: '创建偏好主张' }],
  health: { status: 'attention', message: '1 条待确认', pending_count: 1, conflict_count: 0, integrity_errors: [] },
  context_options: { spaces: [{ id: 'work/hemory', label: 'Hemory' }], purposes: [{ id: 'planning', label: '规划' }], providers: [{ id: 'openai', label: 'OpenAI' }], models: [{ id: 'gpt-5', label: 'GPT-5', provider_id: 'openai' }] },
})

const receipt: WriteReceipt = { claim_id: 'claim-1', revision_id: 'revision-2', payload_sha256: 'sha-2', effective_status: 'active', conflict: false, projection_rebuilt: true }

async function settle() { await Promise.resolve(); await Promise.resolve(); await tick(); flushSync() }
function button(label: string) { return Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((item) => item.textContent?.trim() === label) }
function tab(label: string) { return Array.from(document.querySelectorAll<HTMLButtonElement>('[role=tab]')).find((item) => item.textContent?.includes(label))! }
function rpcMock(implementation: (method: string, params?: any) => Promise<any>) { return vi.fn(implementation) }

async function render(request: ReturnType<typeof rpcMock>) {
  window.notemd = { pluginId: 'notemd.memory', locale: 'zh', theme: 'system', request, onMessage: () => {} }
  const { default: App } = await import('./App.svelte')
  component = mount(App, { target: document.body }); flushSync(); await settle()
}

describe('Memory Protocol v2 app', () => {
  it('copies the cross-assistant import prompt to the clipboard', async () => {
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? baseSnapshot() : {})
    await render(request)
    button('复制导入记忆Prompt')!.click()
    await settle()
    const call = request.mock.calls.find(([method]) => method === 'host.clipboard.write')
    expect(call).toBeTruthy()
    const text = call![1].text as string
    expect(text).toContain('notemd memory propose create')
    expect(text).toContain('--recorded-by')
    expect(text).toContain('不要调用 notemd memory approve / reject / delete')
    expect(request.mock.calls.some(([method]) => method === 'host.memory.v2.add')).toBe(false)
  })

  it('presents confirmed claims with explicit subject, assertion, approval and context semantics', async () => {
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? baseSnapshot() : {})
    await render(request)
    expect(document.body.textContent).toContain('忠实表达本人')
    expect(document.body.textContent).toContain('Vault 所有者')
    expect(document.body.textContent).toContain('work/hemory · planning、writing')
    expect(document.body.textContent).toContain('not-assessed · unknown')
    expect(document.querySelector('[role=tablist]')).toBeTruthy()
    expect(document.querySelector('[role=listbox]')?.getAttribute('tabindex')).toBe('0')
    button('更多')!.click(); flushSync()
    expect(document.querySelector('.menu-panel[role=menu] .menu-row[role=menuitem]')).toBeTruthy()
  })

  it('approves a pending self-representation with one click and no caller-forged human fields', async () => {
    let snapshotCalls = 0
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') { snapshotCalls += 1; return snapshotCalls === 1 ? baseSnapshot() : { ...baseSnapshot(), pending: [] } }
      if (method === 'host.memory.v2.approve') return receipt
      return {}
    })
    await render(request); tab('待确认').click(); flushSync()
    expect(document.body.textContent).toContain('不验证外部事实，也不授权现实行动')
    button('确认记住')!.click(); await settle()
    expect(document.querySelector('[role=alertdialog]')).toBeNull()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({ revision_id: 'pending-1', expected_sha256: 'sha-pending-1', gesture_intent: 'approve', expected_protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' } })
    expect(calls[0][1]).not.toHaveProperty('actor')
    expect(calls[0][1]).not.toHaveProperty('human_confirmed')
  })

  it('edits a confirmed claim through one exact replace RPC and keeps the draft after a stale-base failure', async () => {
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') return baseSnapshot()
      if (method === 'host.memory.v2.replace') throw new Error('MEMORY_STALE_BASE')
      return {}
    })
    await render(request)
    button('更多')!.click(); flushSync(); button('编辑内容…')!.click(); flushSync()
    const dialog = document.querySelector<HTMLElement>('[role=dialog][aria-labelledby=edit-title]')!
    const textarea = dialog.querySelector<HTMLTextAreaElement>('textarea')!
    expect(textarea.value).toBe('回答先给出结论。\n不确定内容应明确标注。')
    textarea.value = '回答应先给出准确结论。'; textarea.dispatchEvent(new Event('input', { bubbles: true })); await settle()
    button('保存修改')!.click(); await settle()

    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.replace')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({
      claim_id: 'claim-1',
      expected_heads: [{ revision_id: 'revision-1', payload_sha256: 'sha-revision-1' }],
      expected_protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' },
      gesture_intent: 'replace',
      text: '回答应先给出准确结论。',
    })
    expect(calls[0][1]).not.toHaveProperty('actor')
    expect(document.querySelector('[role=dialog][aria-labelledby=edit-title]')).toBeTruthy()
    expect(document.querySelector<HTMLTextAreaElement>('[role=dialog][aria-labelledby=edit-title] textarea')?.value).toBe('回答应先给出准确结论。')
    expect(document.querySelector('[role=alert]')?.textContent).toContain('另一设备发生变化')
  })

  it('edits a pending suggestion by approving the corrected text without a second write', async () => {
    let snapshotCalls = 0
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') { snapshotCalls += 1; return snapshotCalls === 1 ? baseSnapshot() : { ...baseSnapshot(), pending: [] } }
      if (method === 'host.memory.v2.approve') return receipt
      return {}
    })
    await render(request); tab('待确认').click(); flushSync(); button('编辑后确认…')!.click(); flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('[role=dialog][aria-labelledby=edit-title] textarea')!
    expect(textarea.value).toContain('回答先给出结论')
    textarea.value = '用户偏好回答先给准确结论。'; textarea.dispatchEvent(new Event('input', { bubbles: true })); await settle()
    button('保存并确认')!.click(); await settle()

    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({
      revision_id: 'pending-1', expected_sha256: 'sha-pending-1', expected_heads: [],
      gesture_intent: 'approve', text_override: '用户偏好回答先给准确结论。',
    })
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.replace')).toHaveLength(0)
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.add')).toHaveLength(0)
    expect(document.querySelector('[role=dialog][aria-labelledby=edit-title]')).toBeNull()
  })

  it('labels behavioral authorization differently and pins it in the same approval RPC', async () => {
    const boundary = pending({ claim_kind: 'boundary', risk_class: 'action-sensitive', text: '不要向第三方发送内容。', agent_use: { guidance: '先询问用户。', avoid_error: '禁止发送。' } })
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? { ...baseSnapshot(), pending: [boundary] } : method === 'host.memory.v2.approve' ? receipt : {})
    await render(request); tab('待确认').click(); flushSync()
    expect(button('允许此行为')).toBeTruthy()
    button('认为重要')!.click(); await settle()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({ salience_override: 'pinned', gesture_intent: 'approve' })
  })

  it('makes a revoke proposal explicit and requires destructive confirmation', async () => {
    const revoked = pending({ lifecycle: { state: 'revoked' } })
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? { ...baseSnapshot(), pending: [revoked] } : method === 'host.memory.v2.approve' ? receipt : {})
    await render(request); tab('待确认').click(); flushSync()
    expect(document.body.textContent).toContain('这是生命周期变更')
    expect(document.body.textContent).toContain('离开当前记忆、纯文本投影和 Agent context')
    expect(button('认为重要')).toBeUndefined()
    expect(button('编辑后确认…')).toBeUndefined()
    button('确认撤销…')!.click(); flushSync()
    expect(document.querySelector('[role=alertdialog]')).toBeTruthy()
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')).toHaveLength(0)
    button('确认')!.click(); await settle()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({ revision_id: 'pending-1', expected_sha256: 'sha-pending-1', gesture_intent: 'approve' })
  })

  it('saves a human-authored claim in one add call and keeps the draft after failure', async () => {
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') return baseSnapshot()
      if (method === 'host.memory.v2.add') throw new Error('MEMORY_STALE_BASE')
      return {}
    })
    await render(request); button('添加主张')!.click(); flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea[placeholder*="原子"]') ?? document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '我希望代码评审先指出风险。'; textarea.dispatchEvent(new Event('input', { bubbles: true })); await settle()
    button('保存并确认')!.click(); await settle()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.add')
    expect(calls).toHaveLength(1)
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')).toHaveLength(0)
    expect(calls[0][1]).toMatchObject({ text: '我希望代码评审先指出风险。', approval_kind: 'self-representation', subject: { kind: 'vault-owner', id: 'owner-1' } })
    expect(document.querySelector('[role=dialog]')).toBeTruthy()
    expect(document.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('我希望代码评审先指出风险。')
    expect(document.querySelector('[role=alert]')?.textContent).toContain('另一设备发生变化')
  })

  it('closes a successful human draft after exactly one atomic add and reports projection recovery separately', async () => {
    const delayedProjection = { ...receipt, projection_rebuilt: false }
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? baseSnapshot() : method === 'host.memory.v2.add' ? delayedProjection : {})
    await render(request); button('添加主张')!.click(); flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '我偏好短而精确的状态更新。'; textarea.dispatchEvent(new Event('input', { bubbles: true })); await settle()
    button('保存并确认')!.click(); await settle()
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.add')).toHaveLength(1)
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')).toHaveLength(0)
    expect(document.querySelector('[role=dialog]')).toBeNull()
    expect(document.body.textContent).toContain('纯文本投影等待重建')
  })

  it('ignores a pending suggestion with one dedicated write and no approval request', async () => {
    const ignored = { ...receipt, effective_status: 'ignored' }
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? baseSnapshot() : method === 'host.memory.v2.ignore' ? ignored : {})
    await render(request); tab('待确认').click(); flushSync(); button('可以忽略')!.click(); await settle()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.ignore')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({ revision_id: 'pending-1', expected_sha256: 'sha-pending-1', gesture_intent: 'ignore' })
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.approve')).toHaveLength(0)
  })

  it('warns about the full impact before resetting every current and pending memory in one RPC', async () => {
    let snapshotCalls = 0
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') {
        snapshotCalls += 1
        return snapshotCalls === 1 ? baseSnapshot() : { ...baseSnapshot(), claims: [], pending: [] }
      }
      if (method === 'host.memory.v2.resetAll') return { deleted_claims: 1, deleted_pending: 1, projection_rebuilt: true, inference_state_reset: true }
      if (method === 'host.vault.exists') return { exists: false }
      return {}
    })
    await render(request)
    button('重构记忆…')!.click(); flushSync()

    expect(document.querySelector('[role=alertdialog]')).toBeTruthy()
    expect(document.body.textContent).toContain('1 条已确认记忆和 1 条待确认建议')
    expect(document.body.textContent).toContain('USER.md、MEMORY.md 与 Agent context')
    expect(document.body.textContent).toContain('所有者身份、Memory 协议和不可变历史会保留')
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.resetAll')).toHaveLength(0)

    button('确认清空并重构')!.click(); await settle()
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.resetAll')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({
      expected_protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' },
      gesture_intent: 'reset-all',
      expected_claims: [{ claim_id: 'claim-1', expected_heads: [{ revision_id: 'revision-1', payload_sha256: 'sha-revision-1' }] }],
      expected_pending: [{ revision_id: 'pending-1', expected_sha256: 'sha-pending-1', expected_heads: [] }],
    })
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.delete')).toHaveLength(0)
    expect(document.querySelector('[role=alertdialog]')).toBeNull()
    expect(document.body.textContent).toContain('下次将重新全量推理')
  })

  it('previews a scoped provider context and writes a manifest only after the explicit action', async () => {
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') return baseSnapshot()
      if (method === 'host.memory.v2.context') return { request: {}, preview_sha256: 'preview-sha', selected: [{ claim_id: 'claim-1', revision_id: 'revision-1', reasons: ['space-match'], text: '回答先给出结论。' }], excluded_summary: { 'provider-deny': 2 }, conflicts: [], redactions: 1, policy_result: { external_action_allowed: false } }
      if (method === 'host.memory.v2.contextManifest') return { manifest_id: 'manifest-1', payload_sha256: 'manifest-sha', selected_count: 1 }
      return {}
    })
    await render(request); button('Context Manifest…')!.click(); flushSync(); button('预览选择')!.click(); await settle()
    expect(document.body.textContent).toContain('1 条将被选中')
    expect(document.body.textContent).toContain('provider-deny 2')
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.contextManifest')).toHaveLength(0)
    button('记录本次使用清单')!.click(); await settle()
    const call = request.mock.calls.find(([method]) => method === 'host.memory.v2.contextManifest')!
    expect(call[1]).toMatchObject({ space: 'work/hemory', purpose: 'planning', provider: 'openai', model: 'gpt-5', caller: 'plugin:notemd.memory', external_transfer: true, preview_sha256: 'preview-sha' })
    expect(call[1].as_of_valid_time).toMatch(/^\d{4}-\d{2}-\d{2}T/)
  })

  it('shows concurrent heads and resolves against every exact head in one RPC', async () => {
    const headA = revision({ revision_id: 'head-a', payload_sha256: 'sha-a', text: '版本 A' })
    const headB = revision({ revision_id: 'head-b', payload_sha256: 'sha-b', text: '版本 B', recorded_by: { kind: 'agent', id: 'agent:b' } })
    const conflict = { conflict_id: 'conflict-1', claim_id: 'claim-1', risk_class: 'action-sensitive' as const, action_allowed: false, common_ancestor: revision({ revision_id: 'ancestor', text: '共同版本' }), heads: [headA, headB], reasons: ['concurrent-heads'] }
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? { ...baseSnapshot(), conflicts: [conflict], health: { ...baseSnapshot().health, conflict_count: 1, status: 'conflict' } } : method === 'host.memory.v2.resolve' ? receipt : {})
    await render(request); tab('冲突与历史').click(); flushSync()
    expect(document.body.textContent).toContain('最后共同版本')
    expect(document.body.textContent).toContain('禁止行动')
    button('采用批准决定')!.click(); await settle()
    const call = request.mock.calls.find(([method]) => method === 'host.memory.v2.resolve')!
    expect(call[1]).toMatchObject({ conflict_id: 'conflict-1', strategy: 'keep-head', selected_revision_id: 'head-a', expected_heads: [{ revision_id: 'head-a', payload_sha256: 'sha-a' }, { revision_id: 'head-b', payload_sha256: 'sha-b' }] })
  })

  it('shows an uninitialized v2 Vault as read-only without any compatibility or migration action', async () => {
    const uninitialized: MemorySnapshotV2 = { mode: 'read-only', initialization_required: true, read_only_reason: '尚未初始化 Memory Protocol v2', claims: [], pending: [], conflicts: [], history: [], health: { status: 'attention', message: '尚未初始化 Memory Protocol v2', pending_count: 0, conflict_count: 0, integrity_errors: [] } }
    let initialized = false
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.initialize') { initialized = true; return {} }
      if (method === 'host.memory.v2.snapshot') return initialized ? { ...baseSnapshot(), claims: [], pending: [], history: [] } : uninitialized
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') return { run_id: 'run-initial' }
      return {}
    })
    await render(request)
    expect(document.body.textContent).toContain('尚未初始化 Memory Protocol v2')
    expect(button('推理现有记忆')?.disabled).toBe(false)
    expect(document.body.textContent).not.toContain('迁移')
    expect(request.mock.calls.some(([method]) => method.includes('migrate'))).toBe(false)
    button('推理现有记忆')!.click(); await settle()
    await vi.waitFor(() => expect(request.mock.calls.some(([method]) => method === 'host.agent.run')).toBe(true))
    const methods = request.mock.calls.map(([method]) => method)
    expect(methods.indexOf('host.memory.v2.initialize')).toBeLessThan(methods.indexOf('host.agent.run'))
  })

  it.each(['read-only', 'recovery'] as const)('keeps %s snapshots visible but blocks writes', async (mode) => {
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? { ...baseSnapshot(), mode, read_only_reason: 'Git 正在合并，写入已暂停' } : {})
    await render(request)
    expect(document.body.textContent).toContain(mode === 'recovery' ? '恢复模式' : '只读模式')
    expect(document.body.textContent).toContain('Git 正在合并')
    expect(button('添加主张')?.disabled).toBe(true)
    expect(document.body.textContent).toContain('回答先给出结论')
  })

  it('starts a full inference when no successful checkpoint exists, even with hand-authored memory', async () => {
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') return baseSnapshot()
      if (method === 'host.agent.providers') return { providers: [{ id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true } }], default: 'notemd.codex-agent' }
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') return { run_id: 'run-memory-1' }
      return {}
    })
    await render(request)
    expect(button('推理现有记忆')).toBeTruthy()
    button('推理现有记忆')!.click(); await settle()
    await vi.waitFor(() => expect(request.mock.calls.some(([method]) => method === 'host.agent.run')).toBe(true))
    const calls = request.mock.calls.filter(([method]) => method === 'host.agent.run')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({ task: 'memory-inference', harness: 'notemd.codex-agent' })
    expect(calls[0][1].prompt).toContain('Mode: full')
    expect(button('正在推理…')?.disabled).toBe(true)
  })

  it('labels the action incremental only after a valid successful checkpoint', async () => {
    const request = rpcMock(async (method, params) => {
      if (method === 'host.memory.v2.snapshot') return { ...baseSnapshot(), claims: [], pending: [], history: [] }
      if (method === 'host.agent.providers') return { providers: [], default: '' }
      if (method === 'host.vault.exists' && params?.path === MEMORY_INFERENCE_STATE) return { exists: true }
      if (method === 'host.vault.read' && params?.path === MEMORY_INFERENCE_STATE) return { content: JSON.stringify({ schema: 'notemd.memory/inference-state/v2', invocation_id: 'old', last_successful_head: 'abc123', complete: true }) }
      return { exists: false }
    })
    await render(request)
    expect(button('增量推理记忆')).toBeTruthy()
  })

  it('treats a successful Agent process without this invocation checkpoint as inference failure', async () => {
    let snapshotCalls = 0
    const request = rpcMock(async (method) => {
      if (method === 'host.memory.v2.snapshot') { snapshotCalls += 1; return baseSnapshot() }
      if (method === 'host.agent.providers') return { providers: [{ id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true } }], default: 'notemd.codex-agent' }
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') return { run_id: 'run-without-checkpoint' }
      if (method === 'host.agent.status') return { state: 'done', record: { status: 'success', result: 'propose 写入失败，未更新 checkpoint' } }
      return {}
    })
    await render(request)
    vi.useFakeTimers()
    button('推理现有记忆')!.click(); await settle()
    await vi.advanceTimersByTimeAsync(2_000); await settle()

    expect(document.querySelector('[role=alert]')?.textContent).toContain('记忆推理失败')
    expect(document.body.textContent).toContain('propose 写入失败')
    expect(snapshotCalls).toBeGreaterThanOrEqual(2)
    expect(request.mock.calls).toContainEqual(['host.toast', expect.objectContaining({ level: 'error', message: '记忆推理失败' })])
  })

  it('reports inference success only when the checkpoint matches this invocation', async () => {
    let invocationId = ''
    let started = false
    const request = rpcMock(async (method, params) => {
      if (method === 'host.memory.v2.snapshot') return baseSnapshot()
      if (method === 'host.agent.providers') return { providers: [{ id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true } }], default: 'notemd.codex-agent' }
      if (method === 'host.vault.exists') return { exists: started && params?.path === MEMORY_INFERENCE_STATE }
      if (method === 'host.vault.read') return { content: JSON.stringify({ schema: 'notemd.memory/inference-state/v2', invocation_id: invocationId, last_successful_head: 'abc123', complete: true }) }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') {
        invocationId = String(params.prompt).match(/Invocation-ID: (.+)/)?.[1] ?? ''
        started = true
        return { run_id: 'run-with-checkpoint' }
      }
      if (method === 'host.agent.status') return { state: 'done', record: { status: 'success', result: '执行完毕' } }
      return {}
    })
    await render(request)
    vi.useFakeTimers()
    button('推理现有记忆')!.click(); await settle()
    await vi.advanceTimersByTimeAsync(2_000); await settle()

    expect(invocationId).not.toBe('')
    expect(document.body.textContent).toContain('记忆推理完成；新建议已放入待确认。')
    expect(request.mock.calls).toContainEqual(['host.toast', expect.objectContaining({ level: 'success' })])
  })

  it('fails closed with a clear upgrade message when the Host returns a non-plugin v2 snapshot shape', async () => {
    const request = rpcMock(async (method) => method === 'host.memory.v2.snapshot' ? { protocol: { heads: [], writable: false }, claims: [] } : {})
    await render(request)
    expect(document.querySelector('[role=alert]')?.textContent).toContain('宿主尚未提供 Memory Protocol v2 RPC')
    expect(button('添加主张')).toBeUndefined()
  })
})
