// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import ConfirmSheet from './ConfirmSheet.svelte'
import type { MemoryEntry, Proposal } from './types'

let component: ReturnType<typeof mount> | null = null
afterEach(() => { if (component) unmount(component); component = null; document.body.innerHTML = '' })

const entry: MemoryEntry = {
  id:'e1', scope:'memory', section:'Active memory', text:'旧内容', revision:2, status:'active', priority:'high',
  polarity:'negative', epistemic_status:'owner-stated', certainty:'high', agent_guidance:'遵守边界',
  avoid_error:'不要泄露', classification_complete:true, document:'MEMORY.md', legacy:false,
}
const proposal: Proposal = {
  type:'Memory Proposal', title:'更新边界', created:'2026-09-01T00:00:00Z',
  proposal:{ version:1,id:'p1',scope:'memory',operation:'replace',target_id:'e1',base_revision:2,
    suggested_priority:'critical',suggested_polarity:'negative',suggested_epistemic_status:'owner-stated',
    suggested_certainty:'high',suggested_agent_guidance:'严格遵守边界',suggested_avoid_error:'禁止泄露',
    dedupe_key:'k',action_sensitive:true,merge_from:[] },
  generated:{by:'agent/x',at:'2026-09-01T00:00:00Z'},sources:[],text:'新内容',reason:'',path:'p.md',sha256:'abc123',decision:'pending',
}

describe('ConfirmSheet', () => {
  it('shows exact identity and does nothing until the explicit decision button is clicked', () => {
    const onconfirm=vi.fn(), oncancel=vi.fn()
    component=mount(ConfirmSheet,{target:document.body,props:{proposal,entries:[entry],action:'approve',oncancel,onconfirm}})
    flushSync()
    const dialog=document.querySelector('[role="alertdialog"]')!
    expect(dialog.textContent).toContain('p1')
    expect(dialog.textContent).toContain('abc123')
    expect(dialog.textContent).toContain('旧内容')
    expect(dialog.textContent).toContain('新内容')
    expect(dialog.textContent).toContain('遵守边界')
    expect(dialog.textContent).toContain('严格遵守边界')
    expect(dialog.textContent).toContain('不要泄露')
    expect(dialog.textContent).toContain('禁止泄露')
    expect(onconfirm).not.toHaveBeenCalled()
    Array.from(dialog.querySelectorAll('button')).find((button)=>button.textContent?.includes('确认并写入'))!.click()
    expect(onconfirm).toHaveBeenCalledOnce()
    expect(oncancel).not.toHaveBeenCalled()
  })

  it('cancels without deciding and disables both controls while busy', () => {
    const onconfirm=vi.fn(), oncancel=vi.fn()
    component=mount(ConfirmSheet,{target:document.body,props:{proposal,entries:[entry],action:'reject',busy:true,oncancel,onconfirm}})
    flushSync()
    expect(Array.from(document.querySelectorAll('button')).every((button)=>button.disabled)).toBe(true)
    unmount(component); component=null; document.body.innerHTML=''
    component=mount(ConfirmSheet,{target:document.body,props:{proposal,entries:[entry],action:'reject',oncancel,onconfirm}})
    flushSync()
    document.querySelector<HTMLButtonElement>('.secondary')!.click()
    expect(oncancel).toHaveBeenCalledOnce()
    expect(onconfirm).not.toHaveBeenCalled()
  })
})
