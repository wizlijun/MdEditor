// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import type { MemoryEntry, Proposal, Snapshot } from './lib/types'

let component: ReturnType<typeof mount> | null = null
afterEach(() => { if (component) unmount(component); component=null; document.body.innerHTML='' })

const entry: MemoryEntry = { id:'e1',scope:'memory',section:'Active memory',text:'尊重隐私边界',revision:1,status:'active',priority:'critical',polarity:'negative',epistemic_status:'owner-stated',certainty:'high',agent_guidance:'保护隐私',avoid_error:'不得泄露',classification_complete:true,source:'/USER.md#privacy',document:'MEMORY.md',legacy:false }
const proposal: Proposal = { type:'Memory Proposal',title:'加强隐私边界',created:'2026-09-01T00:00:00Z',proposal:{version:1,id:'p1',scope:'memory',operation:'replace',target_id:'e1',base_revision:1,suggested_priority:'critical',suggested_polarity:'negative',suggested_epistemic_status:'owner-stated',suggested_certainty:'high',suggested_agent_guidance:'保护隐私',suggested_avoid_error:'不得泄露',dedupe_key:'k',action_sensitive:true,merge_from:[]},generated:{by:'agent/x',at:'2026-09-01T00:00:00Z'},sources:[{id:'s',resource:'/USER.md#privacy'}],text:'严格尊重隐私边界',reason:'',path:'p.md',sha256:'abc',decision:'pending' }
const snapshot: Snapshot = { entries:[entry],proposals:[proposal],integrity:{managed:true,drift:false,errors:[]},owner_actor:'human:bruce' }

describe('Memory app interactions', () => {
  it('navigates and requires the in-window sheet before sending one decision RPC', async () => {
    const request=vi.fn(async (method:string) => method==='host.memory.list' ? snapshot : method==='host.memory.decide' ? {ok:true} : {})
    window.notemd={pluginId:'notemd.memory',locale:'zh',theme:'system',request,onMessage:()=>{}}
    const { default: App } = await import('./App.svelte')
    component=mount(App,{target:document.body}); flushSync(); await tick(); await Promise.resolve(); flushSync()
    const reviewTab=Array.from(document.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((button)=>button.textContent?.includes('待确认'))!
    reviewTab.click(); flushSync()
    expect(document.body.textContent).toContain('加强隐私边界')
    Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((button)=>button.textContent?.includes('审阅并批准'))!.click(); flushSync()
    expect(document.querySelector('[role="alertdialog"]')).not.toBeNull()
    expect(request.mock.calls.filter(([method])=>method==='host.memory.decide')).toHaveLength(0)
    document.querySelector<HTMLButtonElement>('.secondary')!.click(); flushSync()
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
    Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((button)=>button.textContent?.includes('审阅并批准'))!.click(); flushSync()
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')).find((button)=>button.textContent?.includes('确认并写入'))!.click()
    await Promise.resolve(); await Promise.resolve(); flushSync()
    expect(request.mock.calls.filter(([method])=>method==='host.memory.decide')).toHaveLength(1)
  })
})
