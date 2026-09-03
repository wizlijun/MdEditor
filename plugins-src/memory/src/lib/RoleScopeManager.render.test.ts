// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import RoleScopeManager from './RoleScopeManager.svelte'
import { contextBootstrapPrompt } from './contextBootstrapPrompt'
import type {
  ContextRegistrySnapshot,
  EffectiveClaim,
  MemoryClaimRevision,
} from './types'

let component: ReturnType<typeof mount> | null = null
afterEach(() => { if (component) unmount(component); component = null; document.body.innerHTML = '' })

const claim = (): MemoryClaimRevision => ({
  schema: 'notemd.memory/claim-revision/v2', claim_id: 'claim-1', revision_id: 'revision-1', parents: [],
  claim_kind: 'preference', subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self' },
  asserted_by: [{ kind: 'owner', id: 'owner-1' }], recorded_by: { kind: 'host', id: 'notemd.memory-ui' },
  recorded_at: '2026-09-03T01:00:00Z', text: '先给出结论。',
  projection: { target: 'user', category: 'preferences', visibility: 'projection' },
  workflow: { state: 'approved' }, lifecycle: { state: 'active' }, temporal: {},
  epistemic: { basis: 'owner-stated', representation_certainty: 'high', truth_status: 'not-assessed', truth_confidence: 'unknown' },
  trust_tier: 'stable-preference', risk_class: 'informational', salience: 'normal', polarity: 'positive', sensitivity: 'normal',
  context: { spaces: ['scope:product/notemd'], roles: ['role:developer'], applies_when: [], excludes_when: [] },
  consent: { scope: 'personal-assistant-only', allowed_purposes: ['planning'], external_provider_policy: 'prompt' },
  agent_use: { guidance: '', avoid_error: '' }, payload_sha256: 'claim-sha',
})

const claims: EffectiveClaim[] = [{ claim: claim(), application_state: 'current' }]

const baseRegistry = (): ContextRegistrySnapshot => ({
  protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' },
  registry_heads: [{ revision_id: 'registry-1', payload_sha256: 'registry-sha' }],
  roles: [
    { id: 'role:developer', label: '开发者', description: '开发产品', aliases: ['开发'], status: 'active', guidance: '重视可验证性', avoid_error: '不要跳过测试' },
    { id: 'role:unclassified', label: '未分类', description: '', aliases: [], status: 'active', guidance: '', avoid_error: '' },
  ],
  scopes: [{ id: 'scope:product/notemd', label: 'note.md', description: '自有产品', aliases: [], status: 'active', kind: 'realm', security_domain: 'product/notemd' }],
  writable: true,
})

async function settle() { await Promise.resolve(); await Promise.resolve(); await tick(); flushSync() }
function button(label: string) { return Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((item) => item.textContent?.trim() === label) }
function input(placeholder: string) { return document.querySelector<HTMLInputElement>(`input[placeholder="${placeholder}"]`)! }
function change(control: HTMLInputElement, value: string) { control.value = value; control.dispatchEvent(new Event('input', { bubbles: true })); flushSync() }

async function render(request: (method: string, params?: any) => Promise<any>, onchanged = vi.fn()) {
  window.notemd = { pluginId: 'notemd.memory', locale: 'zh', theme: 'system', request, onMessage: () => {} }
  component = mount(RoleScopeManager, { target: document.body, props: { claims, onchanged } })
  flushSync()
  await settle()
  return onchanged
}

describe('RoleScopeManager', () => {
  it('renders as an inline workspace instead of a modal sheet', async () => {
    const request = vi.fn(async (method: string, _params?: any) => method === 'host.memory.v2.contextRegistry' ? baseRegistry() : {})
    await render(request)

    expect(document.querySelector('section[aria-labelledby="role-scope-title"]')).toBeTruthy()
    expect(document.querySelector('[role=dialog]')).toBeNull()
    expect(document.querySelector('.scrim')).toBeNull()
    expect(button('关闭身份与场景')).toBeUndefined()
  })

  it('copies a staged initialization prompt without mutating Memory while copying', async () => {
    const request = vi.fn(async (method: string, _params?: any) => method === 'host.memory.v2.contextRegistry' ? baseRegistry() : {})
    await render(request)

    button('复制初始化 Prompt')!.click(); await settle()

    const clipboardCall = request.mock.calls.find(([method]) => method === 'host.clipboard.write')
    expect(clipboardCall?.[1]).toEqual({ text: contextBootstrapPrompt })
    expect(contextBootstrapPrompt).toContain('notemd memory context-registry show --json')
    expect(contextBootstrapPrompt).toContain('notemd memory context-registry validate --file <临时候选文件> --json')
    expect(contextBootstrapPrompt).toContain('notemd memory context-registry replace')
    expect(contextBootstrapPrompt).toContain('notemd memory reassign plan')
    expect(contextBootstrapPrompt).toContain('notemd memory reassign propose')
    expect(contextBootstrapPrompt).toContain('Registry 更新成功并精确核对后')
    expect(contextBootstrapPrompt).toContain('不得强行归类')
    expect(contextBootstrapPrompt).not.toMatch(/^notemd memory context-registry (?:apply|import)/m)
    expect(contextBootstrapPrompt).not.toMatch(/^notemd memory reassign apply/m)
    expect(request.mock.calls.some(([method]) => method === 'host.memory.v2.contextRegistryReplace')).toBe(false)
    expect(request.mock.calls.some(([method]) => method === 'host.memory.v2.reassignApply')).toBe(false)
    expect(request.mock.calls).toContainEqual(['host.toast', expect.objectContaining({ level: 'success', message: '已复制身份与场景初始化 Prompt' })])
  })

  it('keeps the workspace open and reports clipboard failures inline', async () => {
    const request = vi.fn(async (method: string, _params?: any) => {
      if (method === 'host.memory.v2.contextRegistry') return baseRegistry()
      if (method === 'host.clipboard.write') throw new Error('clipboard permission denied')
      return {}
    })
    await render(request)

    button('复制初始化 Prompt')!.click(); await settle()

    expect(document.querySelector('[role=alert]')?.textContent).toContain('clipboard permission denied')
    expect(document.querySelector('section[aria-labelledby="role-scope-title"]')).toBeTruthy()
    expect(request.mock.calls.some(([method]) => method === 'host.toast')).toBe(false)
  })

  it('creates a Role with one full-registry replacement and exact concurrency heads', async () => {
    const registry = baseRegistry()
    const request = vi.fn(async (method: string, _params?: any) => method === 'host.memory.v2.contextRegistry' ? registry : {})
    await render(request)

    button('新增 Role')!.click(); flushSync()
    change(input('role:developer'), 'role:consultant')
    change(input('开发者'), '顾问')
    button('保存 Role')!.click(); await settle()

    await vi.waitFor(() => expect(request.mock.calls.some(([method]) => method === 'host.memory.v2.contextRegistryReplace')).toBe(true))
    const calls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.contextRegistryReplace')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toMatchObject({
      expected_protocol: registry.protocol,
      expected_registry_heads: registry.registry_heads,
      gesture_intent: 'replace-context-registry',
      scopes: registry.scopes,
      roles: expect.arrayContaining([expect.objectContaining({ id: 'role:consultant', label: '顾问', status: 'active' })]),
    })
    expect(calls[0][1].request_id).toMatch(/^memory-ui\/context-registry\//)
  })

  it('requires explicit confirmation for archive and supports restoring the same stable Role', async () => {
    let registry = baseRegistry()
    const request = vi.fn(async (method: string, params?: any) => {
      if (method === 'host.memory.v2.contextRegistry') return registry
      if (method === 'host.memory.v2.contextRegistryReplace') {
        registry = { ...registry, roles: params.roles, scopes: params.scopes }
        return {}
      }
      return {}
    })
    await render(request)

    button('归档…')!.click(); flushSync()
    expect(document.querySelector('[role=alertdialog]')).toBeTruthy()
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.contextRegistryReplace')).toHaveLength(0)
    button('确认归档')!.click(); await settle()
    await vi.waitFor(() => expect(registry.roles[0].status).toBe('archived'))

    await vi.waitFor(() => expect(button('恢复…')?.disabled).toBe(false))
    button('恢复…')!.click(); flushSync()
    button('确认恢复')!.click(); await settle()
    await vi.waitFor(() => expect(registry.roles[0].status).toBe('active'))
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.contextRegistryReplace')).toHaveLength(2)
  })

  it('previews exact selected claims and applies the frozen batch with one RPC', async () => {
    const registry = baseRegistry()
    const preview = {
      preview_sha256: 'preview-sha',
      matched: [{ claim_id: 'claim-1', expected_heads: [{ revision_id: 'revision-1', payload_sha256: 'claim-sha' }], before: { roles: ['role:developer'] }, after: { roles: ['role:developer'], scopes: ['scope:product/notemd'] }, risk_bucket: 'same-realm', batch_eligible: true, reasons: ['same-security-domain'] }],
      summary: { matched: 1 },
    }
    const onchanged = vi.fn()
    const request = vi.fn(async (method: string, _params?: any) => {
      if (method === 'host.memory.v2.contextRegistry') return registry
      if (method === 'host.memory.v2.reassignPreview') return preview
      if (method === 'host.memory.v2.reassignApply') return { updated_claims: 1, projection_rebuilt: true }
      return {}
    })
    await render(request, onchanged)
    button('重新分配')!.click(); flushSync()
    document.querySelector<HTMLInputElement>('[aria-label="选择主张 claim-1"]')!.click()
    document.querySelector<HTMLInputElement>('[aria-label="目标 Scope scope:product/notemd"]')!.click()
    flushSync()
    button('预览重新分配')!.click(); await settle()

    const previewCall = request.mock.calls.find(([method]) => method === 'host.memory.v2.reassignPreview')!
    expect(previewCall[1]).toMatchObject({
      expected_protocol: registry.protocol,
      expected_registry_heads: registry.registry_heads,
      selector: { claim_ids: ['claim-1'] },
      replacement: { scope_ids: ['scope:product/notemd'] },
    })
    expect(request.mock.calls.some(([method]) => method === 'host.memory.v2.reassignApply')).toBe(false)

    button('应用 1 条重新分配')!.click(); await settle()
    const applyCalls = request.mock.calls.filter(([method]) => method === 'host.memory.v2.reassignApply')
    expect(applyCalls).toHaveLength(1)
    expect(applyCalls[0][1]).toMatchObject({
      ...previewCall[1],
      preview_sha256: 'preview-sha',
      gesture_intent: 'apply-reassignment',
    })
    expect(applyCalls[0][1].request_id).toMatch(/^memory-ui\/reassignment\//)
    await vi.waitFor(() => expect(onchanged).toHaveBeenCalledTimes(1))
    expect(request.mock.calls.filter(([method]) => method === 'host.memory.v2.replace')).toHaveLength(0)
  })

  it('keeps selections and targets after a stale apply but invalidates the old preview', async () => {
    const registry = baseRegistry()
    const request = vi.fn(async (method: string, _params?: any) => {
      if (method === 'host.memory.v2.contextRegistry') return registry
      if (method === 'host.memory.v2.reassignPreview') return {
        preview_sha256: 'preview-sha', summary: {},
        matched: [{ claim_id: 'claim-1', expected_heads: [], before: {}, after: {}, risk_bucket: 'same-realm', batch_eligible: true, reasons: [] }],
      }
      if (method === 'host.memory.v2.reassignApply') throw new Error('MEMORY_STALE_BASE: claim changed')
      return {}
    })
    await render(request)
    button('重新分配')!.click(); flushSync()
    const claimBox = document.querySelector<HTMLInputElement>('[aria-label="选择主张 claim-1"]')!
    const scopeBox = document.querySelector<HTMLInputElement>('[aria-label="目标 Scope scope:product/notemd"]')!
    claimBox.click(); scopeBox.click(); flushSync()
    button('预览重新分配')!.click(); await settle(); button('应用 1 条重新分配')!.click(); await settle()

    expect(claimBox.checked).toBe(true)
    expect(scopeBox.checked).toBe(true)
    expect(document.body.textContent).toContain('另一设备发生变化')
    expect(button('预览重新分配')).toBeTruthy()
    expect(button('应用 1 条重新分配')).toBeUndefined()
  })
})
