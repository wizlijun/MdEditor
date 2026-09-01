// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import type { MemoryEntry, Proposal, Snapshot } from './lib/types'

let component: ReturnType<typeof mount> | null = null
afterEach(() => { if (component) unmount(component); component=null; document.body.innerHTML='' })

const entry: MemoryEntry = { id:'e1',scope:'memory',section:'Active memory',text:'尊重隐私边界',revision:1,status:'active',priority:'critical',polarity:'negative',epistemic_status:'owner-stated',certainty:'high',agent_guidance:'保护隐私',avoid_error:'不得泄露',classification_complete:true,source:'/USER.md#privacy',document:'MEMORY.md',legacy:false }
const proposal: Proposal = { type:'Memory Proposal',title:'加强隐私边界',created:'2026-09-01T00:00:00Z',proposal:{version:1,id:'p1',scope:'memory',operation:'replace',target_id:'e1',base_revision:1,suggested_priority:'critical',suggested_polarity:'negative',suggested_epistemic_status:'owner-stated',suggested_certainty:'high',suggested_agent_guidance:'保护隐私',suggested_avoid_error:'不得泄露',dedupe_key:'k',action_sensitive:true,merge_from:[]},generated:{by:'agent/x',at:'2026-09-01T00:00:00Z'},sources:[{id:'s',resource:'/USER.md#privacy'}],text:'严格尊重隐私边界',reason:'',path:'p.md',sha256:'abc',decision:'pending' }
const snapshot: Snapshot = { entries:[entry],proposals:[proposal],integrity:{managed:true,drift:false,errors:[]},owner_actor:'human:bruce' }

async function settle() { await Promise.resolve(); await Promise.resolve(); flushSync() }
function button(label: string) { return Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((item)=>item.textContent?.trim()===label)! }

describe('Memory app interactions', () => {
  it('approves a reviewed proposal with one manual click and no second sheet', async () => {
    const request=vi.fn(async (method:string, _params?:unknown) => method==='host.memory.list' ? snapshot : method==='host.memory.decide' ? {ok:true} : {})
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()
    const reviewTab=Array.from(document.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((button)=>button.textContent?.includes('待确认'))!
    reviewTab.click(); flushSync()
    expect(document.body.textContent).toContain('加强隐私边界')
    button('批准').click(); await settle()
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
    expect(request.mock.calls.filter(([method])=>method==='host.memory.propose')).toHaveLength(0)
    const decisions=request.mock.calls.filter(([method])=>method==='host.memory.decide')
    expect(decisions).toHaveLength(1)
    expect(decisions[0][1]).toMatchObject({ proposal_id:'p1',expected_sha256:'abc',action:'approve',actor:'human:bruce',human_confirmed:true })
  })

  it('offers five no-form shortcuts and maps important to an exact high-priority proposal', async () => {
    const request=vi.fn(async (method:string, params:any) => {
      if (method==='host.memory.list') return snapshot
      if (method==='host.memory.propose') return { ...proposal, proposal:{ ...proposal.proposal, id:'quick-important', operation:params.operation, suggested_priority:params.priority, dedupe_key:params.dedupe_key }, text:params.text, sha256:'quick-sha' }
      return {}
    })
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    for (const label of ['确认','否认','重要','可忽略','删除…']) expect(button(label)).toBeTruthy()
    button('重要').click(); await settle()

    const call=request.mock.calls.find(([method])=>method==='host.memory.propose')!
    expect(call[1]).toMatchObject({ operation:'set-priority', target_id:'e1', base_revision:1, priority:'high', text:'' })
    expect(call[1].dedupe_key).toBe('memory-ui/quick/v1/important/e1/r1')
    expect(document.querySelector('[aria-label="Memory editor"]')).toBeNull()
    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain('将这条记忆标为重要')
    expect(request.mock.calls.filter(([method])=>method==='host.memory.decide')).toHaveLength(0)
  })

  it('reuses the exact pending candidate for confirm without proposing another change', async () => {
    const pendingSnapshot: Snapshot = { ...snapshot, entries:[{ ...entry, status:'pending', revision:0, proposal:'p1' }] }
    const request=vi.fn(async (method:string, _params?:unknown) => method==='host.memory.list' ? pendingSnapshot : method==='host.memory.decide' ? {ok:true} : {})
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    expect(button('重要')).toBeUndefined()
    expect(button('删除…')).toBeUndefined()
    button('确认').click(); await settle()
    expect(request.mock.calls.filter(([method])=>method==='host.memory.propose')).toHaveLength(0)
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
    const decisions=request.mock.calls.filter(([method])=>method==='host.memory.decide')
    expect(decisions).toHaveLength(1)
    expect(decisions[0][1]).toMatchObject({ proposal_id:'p1',expected_sha256:'abc',action:'approve',actor:'human:bruce',human_confirmed:true })
  })

  it('auto-approves a manual confirm for an active fact after creating its exact proposal', async () => {
    const request=vi.fn(async (method:string, params:any) => {
      if (method==='host.memory.list') return snapshot
      if (method==='host.memory.propose') return { ...proposal, proposal:{ ...proposal.proposal, id:'quick-confirm', operation:params.operation, dedupe_key:params.dedupe_key }, text:params.text, sha256:'confirm-sha' }
      if (method==='host.memory.decide') return {ok:true}
      return {}
    })
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    button('确认').click(); await settle()

    expect(request.mock.calls.filter(([method])=>method==='host.memory.propose')).toHaveLength(1)
    const decisions=request.mock.calls.filter(([method])=>method==='host.memory.decide')
    expect(decisions).toHaveLength(1)
    expect(decisions[0][1]).toMatchObject({ proposal_id:'quick-confirm',expected_sha256:'confirm-sha',action:'approve',actor:'human:bruce',human_confirmed:true })
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
  })

  it('surfaces a one-click approval failure without sending a second decision', async () => {
    const request=vi.fn(async (method:string) => {
      if (method==='host.memory.list') return snapshot
      if (method==='host.memory.decide') throw new Error('proposal content changed')
      return {}
    })
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    const reviewTab=Array.from(document.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((item)=>item.textContent?.includes('待确认'))!
    reviewTab.click(); flushSync()
    button('批准').click(); await settle()

    expect(document.querySelector('[role="alert"]')?.textContent).toContain('proposal content changed')
    expect(request.mock.calls.filter(([method])=>method==='host.memory.decide')).toHaveLength(1)
    expect(button('批准').disabled).toBe(false)
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
  })

  it('keeps current references out of view while preserving proposal evidence during review', async () => {
    const request=vi.fn(async (method:string) => method==='host.memory.list' ? snapshot : {})
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    expect(document.body.textContent).not.toContain('/USER.md#privacy')
    const reviewTab=Array.from(document.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((item)=>item.textContent?.includes('待确认'))!
    reviewTab.click(); flushSync()
    expect(document.body.textContent).toContain('/USER.md#privacy')
  })

  it('shows deletion as a destructive audited projection removal', async () => {
    const request=vi.fn(async (method:string, params:any) => {
      if (method==='host.memory.list') return snapshot
      if (method==='host.memory.propose') return { ...proposal, proposal:{ ...proposal.proposal, id:'quick-delete', operation:'delete', dedupe_key:params.dedupe_key }, text:'', sha256:'delete-sha' }
      return {}
    })
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    button('删除…').click(); await settle()
    const sheet=document.querySelector('[role="alertdialog"]')!
    expect(sheet.textContent).toContain('从当前记忆中删除')
    expect(sheet.textContent).toContain('候选与决定事件仍保留用于审计')
    expect(sheet.querySelector<HTMLButtonElement>('.destructive')?.textContent).toContain('确认删除')
    expect(request.mock.calls.find(([method])=>method==='host.memory.propose')?.[1]).toMatchObject({ operation:'delete', target_id:'e1', base_revision:1, text:'' })
  })

  it('never exposes fact shortcuts for the owner identity record', async () => {
    const owner = { ...entry, id:'owner', scope:'user-owner' as const, text:'human:bruce' }
    const request=vi.fn(async (method:string) => method==='host.memory.list' ? { ...snapshot, entries:[owner], proposals:[] } : {})
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await settle()

    expect(document.querySelector('[aria-label="事实快捷审阅"]')).toBeNull()
    expect(request.mock.calls.filter(([method])=>method==='host.memory.propose')).toHaveLength(0)
  })
})
