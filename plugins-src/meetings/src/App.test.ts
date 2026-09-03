import { mount, tick, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { NotemdBridge } from './lib/bridge'
import type { MigrationReport } from './lib/types'

function button(label: string, root: ParentNode = document): HTMLButtonElement {
  const match = [...root.querySelectorAll<HTMLButtonElement>('button')]
    .find((candidate) => candidate.textContent?.trim() === label)
  if (!match) throw new Error(`button not found: ${label}`)
  return match
}

function migrationReport(patch: Partial<MigrationReport> = {}): MigrationReport {
  return {
    schema_version: 1,
    mode: 'incremental',
    dry_run: true,
    source_user: 'team',
    scanned: 1,
    eligible: 1,
    create: 1,
    update: 0,
    skip: 0,
    conflict: 0,
    blocked: 0,
    excluded_audio: 2,
    committed: 0,
    source_missing: 0,
    warnings: [],
    errors: [],
    items: [{
      conversation_id: '20260903_090000',
      source_relative_path: 'team/conversation/202609/20260903_090000',
      target_relative_path: 'ssot/meetings/20260903_090000',
      selected_transcript: 'pro_asr.srt',
      action: 'create',
    }],
    ...patch,
  }
}

describe('Meetings UI', () => {
  let app: ReturnType<typeof mount> | undefined

  afterEach(async () => {
    if (app) await unmount(app)
    app = undefined
    document.body.innerHTML = ''
    vi.restoreAllMocks()
  })

  it('lists meetings newest first and opens the authoritative transcript', async () => {
    const request = vi.fn(async (method: string) => {
      if (method === 'host.vault.info') return { root: '/vault' }
      if (method === 'plugin.library_list') {
        return {
          meetings: [
            {
              conversation_id: 'older',
              title: 'Older meeting',
              started_at: '2026-09-01T09:00:00+08:00',
              transcript_path: 'ssot/meetings/older/transcript.srt',
            },
            {
              conversation_id: 'newer',
              title: 'Newer meeting',
              started_at: '2026-09-03T09:00:00+08:00',
              transcript_path: 'ssot/meetings/newer/transcript.srt',
            },
          ],
        }
      }
      if (method === 'host.editor.open') return {}
      throw new Error(`unexpected RPC: ${method}`)
    })
    window.notemd = {
      pluginId: 'notemd.meetings',
      locale: 'en',
      theme: 'light',
      request,
      onMessage: () => {},
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    await vi.waitFor(() => expect(document.querySelectorAll('.meeting-row')).toHaveLength(2))

    expect([...document.querySelectorAll('.meeting-row strong')].map((node) => node.textContent)).toEqual([
      'Newer meeting',
      'Older meeting',
    ])
    document.querySelector<HTMLButtonElement>('.meeting-main')?.click()
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('host.editor.open', {
      path: 'ssot/meetings/newer/transcript.srt',
    }))
  })

  it('detects user and timezone, previews, starts, reports progress, and cancels safely', async () => {
    let push: ((payload: unknown) => void) | undefined
    const plans: unknown[] = []
    const expectedPlan = migrationReport()
    const request = vi.fn(async (method: string, params?: unknown) => {
      if (method === 'host.vault.info') return { root: '/vault' }
      if (method === 'plugin.library_list') return { meetings: [] }
      if (method === 'host.dialog.open') return { paths: ['/readonly/hemory'] }
      if (method === 'plugin.hemory_detect') {
        return {
          users: [{ id: 'personal', label: 'Personal' }, { id: 'team', label: 'Team' }],
          needs_timezone: true,
          warnings: ['one legacy layout'],
        }
      }
      if (method === 'plugin.hemory_plan') {
        plans.push(params)
        return expectedPlan
      }
      if (method === 'plugin.hemory_apply_start') return { job_id: 17 }
      if (method === 'plugin.hemory_cancel' || method === 'host.toast') return {}
      throw new Error(`unexpected RPC: ${method}`)
    })
    window.notemd = {
      pluginId: 'notemd.meetings',
      locale: 'en',
      theme: 'light',
      request,
      onMessage: (callback) => { push = callback },
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('No meeting transcripts yet.'))
    button('Migrate from Hemory…').click()
    await tick()
    button('Choose Hemory folder…').click()
    await vi.waitFor(() => expect(document.querySelector<HTMLSelectElement>('#hemory-user')).not.toBeNull())

    const user = document.querySelector<HTMLSelectElement>('#hemory-user')!
    user.value = 'team'
    user.dispatchEvent(new Event('change', { bubbles: true }))
    const timezone = document.querySelector<HTMLInputElement>('#hemory-timezone')!
    timezone.value = 'Asia/Taipei'
    timezone.dispatchEvent(new Event('input', { bubbles: true }))
    await tick()
    button('Run preflight').click()

    await vi.waitFor(() => expect(document.querySelector('.report-card')).not.toBeNull())
    expect(plans).toEqual([{
      source: '/readonly/hemory',
      mode: 'incremental',
      user: 'team',
      timezone: 'Asia/Taipei',
    }])
    expect(document.body.textContent).toContain('pro_asr.srt')

    user.value = ''
    user.dispatchEvent(new Event('change', { bubbles: true }))
    await tick()
    expect(document.querySelector('.report-card')).toBeNull()
    expect(button('Run preflight').disabled).toBe(true)
    user.value = 'team'
    user.dispatchEvent(new Event('change', { bubbles: true }))
    await tick()
    expect(button('Run preflight').disabled).toBe(false)

    timezone.value = 'Asia/Tokyo'
    timezone.dispatchEvent(new Event('input', { bubbles: true }))
    await tick()
    expect(document.querySelector('.report-card')).toBeNull()
    button('Run preflight').click()
    await vi.waitFor(() => expect(plans).toHaveLength(2))
    await vi.waitFor(() => expect(document.querySelector('.report-card')).not.toBeNull())

    button('Start migration').click()
    await tick()
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!
    expect(dialog).not.toBeNull()
    button('Start migration', dialog).click()
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('plugin.hemory_apply_start', {
      source: '/readonly/hemory',
      mode: 'incremental',
      user: 'team',
      timezone: 'Asia/Tokyo',
      expected_plan: expectedPlan,
    }))

    push?.({
      type: 'hemory-migration',
      job_id: 17,
      event: 'progress',
      committed: 1,
      total: 1,
      item: migrationReport().items[0],
    })
    await tick()
    expect(document.body.textContent).toContain('1 of 1 committed')
    expect(document.body.textContent).toContain('20260903_090000')

    button('Stop after this meeting').click()
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('plugin.hemory_cancel', { job_id: 17 }))
    push?.({
      type: 'hemory-migration',
      job_id: 17,
      event: 'cancelled',
      report: migrationReport({ dry_run: false, committed: 1 }),
    })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Migration result'))
    expect(document.body.textContent).toContain('Committed')
  })

  it('automatically detects the first Hemory Vault when migration opens', async () => {
    const report = migrationReport({
      items: [{
        ...migrationReport().items[0],
        target_relative_path: 'archive/meetings/20260903_090000',
      }],
    })
    const request = vi.fn(async (method: string, params?: unknown) => {
      if (method === 'host.vault.info') return { root: '/vault' }
      if (method === 'plugin.detect_env') {
        return {
          settings: { meetings_root: 'archive/meetings' },
          default_hemory_source: '/home/me/.hemory/vault/0001',
        }
      }
      if (method === 'plugin.library_list') return { meetings: [] }
      if (method === 'plugin.hemory_detect') {
        return { users: ['alice'], selected_user: 'alice', needs_timezone: false, warnings: [] }
      }
      if (method === 'plugin.hemory_plan') return report
      throw new Error(`unexpected RPC: ${method} ${JSON.stringify(params)}`)
    })
    window.notemd = {
      pluginId: 'notemd.meetings', locale: 'en', theme: 'light', request, onMessage: () => {},
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('archive/meetings'))
    button('Migrate from Hemory…').click()
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('plugin.hemory_detect', {
      source: '/home/me/.hemory/vault/0001',
    }))
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('plugin.hemory_plan', {
      source: '/home/me/.hemory/vault/0001',
      mode: 'incremental',
      user: 'alice',
    }))
    expect(document.body.textContent).toContain('/home/me/.hemory/vault/0001')
  })

  it('loads and saves the Vault-relative meetings directory from settings', async () => {
    let libraryLoads = 0
    const request = vi.fn(async (method: string) => {
      if (method === 'host.vault.info') return { root: '/vault' }
      if (method === 'plugin.detect_env') {
        return { settings: { meetings_root: 'ssot/meetings' }, default_hemory_source: null }
      }
      if (method === 'plugin.library_list') {
        libraryLoads += 1
        return { meetings: [] }
      }
      if (method === 'plugin.save_settings') return { meetings_root: 'team/transcripts' }
      if (method === 'host.toast') return {}
      throw new Error(`unexpected RPC: ${method}`)
    })
    window.notemd = {
      pluginId: 'notemd.meetings', locale: 'en', theme: 'light', request, onMessage: () => {},
    } satisfies NotemdBridge

    app = mount(App, { target: document.body })
    await vi.waitFor(() => expect(libraryLoads).toBe(1))
    button('Settings').click()
    await tick()
    const input = document.querySelector<HTMLInputElement>('#meetings-root')!
    input.value = 'team/transcripts'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    button('Save settings').click()
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith('plugin.save_settings', {
      meetings_root: 'team/transcripts',
    }))
    await vi.waitFor(() => expect(libraryLoads).toBe(2))
    expect(document.body.textContent).toContain('team/transcripts')
  })
})
