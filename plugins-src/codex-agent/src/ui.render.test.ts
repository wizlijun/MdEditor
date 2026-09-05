import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import App from './App.svelte'
import SettingsPage from './components/SettingsPage.svelte'
import HistoryList from './components/HistoryList.svelte'
import RunLog from './components/RunLog.svelte'
import RunStream from './components/RunStream.svelte'
import { t, type MessageKey } from './lib/strings'
import type { RunRecord } from './lib/events'

const label = (key: MessageKey, values?: Record<string, string | number>) => t('zh', key, values)
const components: ReturnType<typeof mount>[] = []
const record = (id: string): RunRecord => ({
  run_id: id, task: '项目检查', trigger: 'window', started_at: '2026-09-05T01:00:00Z',
  ended_at: '2026-09-05T01:02:00Z', status: 'success', result: '检查完成', stderr_tail: '',
  artifacts: ['projects/report.md'], usage: { input_tokens: 10, output_tokens: 5, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, reported_total_tokens: 15 },
})
async function settle() { await Promise.resolve(); await Promise.resolve(); await tick(); flushSync() }
function button(text: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text)! }
function change(select: HTMLSelectElement, value: string) { select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); flushSync() }
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail }); return { promise, resolve, reject } }
beforeEach(() => {
  window.notemd = {
    pluginId: 'fixture', locale: 'zh', theme: 'system', onMessage: () => {},
    request: vi.fn(async (method: string) => {
      if (method === 'host.settings.get') return { settings: { maxConcurrency: '2', usageDisplay: 'tip' } }
      if (method === 'plugin.tasks.list') return { ready: true, tasks: [{ id: 'check', name: '项目检查', description: '检查本地项目', running: false }] }
      if (method === 'plugin.history.list') return { runs: [record('one'), record('two')] }
      if (method === 'plugin.context.get') return { tab: { path: 'projects/demo.md', selection: '' } }
      if (method === 'plugin.harness-status') return { ok: true, harness: 'Fixture', version: '1' }
      return {}
    }),
  }
})
afterEach(async () => { for (const c of components.splice(0)) await unmount(c); document.body.innerHTML = ''; vi.restoreAllMocks() })

describe('Agent window accessible UI', () => {
  it.each([false, true])('restores async field focus without stealing a later selection (%s)', async (moved) => {
    const pending = deferred<unknown>()
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => method === 'host.settings.set' ? pending.promise : original(method, params))
    components.push(mount(SettingsPage, { target: document.body, props: { label } }))
    flushSync(); await settle()
    const select = document.querySelector<HTMLSelectElement>('#max-concurrency')!
    const other = document.createElement('button'); document.body.append(other)
    await vi.waitFor(() => expect(select.disabled).toBe(false))
    select.focus(); change(select, '3')
    // jsdom does not blur controls when disabled; browsers do.
    select.blur()
    if (moved) other.focus()
    pending.resolve({}); await settle()
    await vi.waitFor(() => expect(document.activeElement).toBe(moved ? other : select))
  })

  it('names settings and hints and reports success only after persistence', async () => {
    const pending = deferred<unknown>()
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => method === 'host.settings.set' ? pending.promise : original(method, params))
    components.push(mount(SettingsPage, { target: document.body, props: { label } }))
    flushSync(); await settle()
    const select = document.querySelector<HTMLSelectElement>('#max-concurrency')!
    expect(select.value).toBe('2')
    expect(select.getAttribute('aria-describedby')).toBe('max-concurrency-hint')
    expect(document.querySelector('label[for=max-concurrency]')?.textContent).toBe(label('settings.maxConcurrency'))
    change(select, '3'); await settle()
    expect(document.querySelector('[role=status]')?.textContent).toBe(label('settings.saving'))
    expect(select.disabled).toBe(true)
    expect(document.body.textContent).not.toContain(label('settings.saved'))
    pending.resolve({}); await settle()
    expect(document.querySelector('[role=status]')?.textContent).toBe(label('settings.saved'))
    expect(select.disabled).toBe(false)
  })

  it('keeps the prior setting on failed save and presents a local error', async () => {
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => method === 'host.settings.set' ? Promise.reject(new Error('disk full')) : original(method, params))
    components.push(mount(SettingsPage, { target: document.body, props: { label } }))
    flushSync(); await settle()
    const select = document.querySelector<HTMLSelectElement>('#usage-display')!
    change(select, 'result'); await settle()
    expect(select.value).toBe('tip')
    expect(document.querySelector('[role=alert]')?.textContent).toBe(label('settings.saveFailed'))
    expect(select.disabled).toBe(false)
  })

  it('allows a failed settings load to be retried without saving guessed values', async () => {
    let fail = true
    window.notemd.request = vi.fn(async () => { if (fail) throw new Error('offline'); return { settings: { maxConcurrency: '4' } } })
    components.push(mount(SettingsPage, { target: document.body, props: { label } }))
    flushSync(); await settle()
    expect(document.querySelector<HTMLSelectElement>('select')?.disabled).toBe(true)
    expect(document.querySelector('[role=alert]')?.textContent).toBe(label('settings.loadFailed'))
    fail = false; button(label('settings.retry')).click(); await settle()
    expect(document.querySelector<HTMLSelectElement>('select')?.value).toBe('4')
    await vi.waitFor(() => expect(document.querySelector<HTMLSelectElement>('select')?.disabled).toBe(false))
  })

  it('opens history actions from the keyboard, confirms deletion, and returns focus', async () => {
    const ondelete = vi.fn(), onclear = vi.fn()
    components.push(mount(HistoryList, { target: document.body, props: { runs: [record('one')], label, empty: '', selectedId: 'one', onselect: vi.fn(), ondelete, onclear } }))
    flushSync()
    const row = document.querySelector<HTMLButtonElement>('.row')!
    expect(row.getAttribute('aria-current')).toBe('page')
    row.focus(); row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true })); await settle()
    expect(document.activeElement?.textContent).toBe(label('history.delete'))
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    expect(document.activeElement?.textContent).toBe(label('history.clearAll'))
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await settle()
    expect(document.querySelector('[role=menu]')).toBeNull()
    expect(document.activeElement).toBe(row)
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true })); await settle()
    button(label('history.delete')).click(); await settle()
    expect(document.querySelector('[role=alertdialog]')).not.toBeNull()
    expect(ondelete).not.toHaveBeenCalled()
    expect(document.activeElement?.textContent).toBe(label('history.cancel'))
    button(label('history.delete')).click(); await settle()
    expect(ondelete).toHaveBeenCalledOnce()
    expect(onclear).not.toHaveBeenCalled()
    expect(document.querySelector('[role=alertdialog]')).toBeNull()
    expect(document.activeElement).toBe(row)
  })

  it('renders task navigation, settings, run history, usage and output links through the real App', async () => {
    components.push(mount(App, { target: document.body })); flushSync(); await settle(); await settle()
    expect(document.querySelector('main.ui-surface')).not.toBeNull()
    document.querySelector<HTMLButtonElement>('.group-toggle')!.click(); await settle()
    expect(document.querySelector('.task-button')?.getAttribute('aria-current')).toBe('page')
    button(label('settings.title')).click(); await settle()
    expect(document.querySelector('.settings-entry')?.getAttribute('aria-current')).toBe('page')
    expect(document.querySelector('.task-button')?.hasAttribute('aria-current')).toBe(false)
    document.querySelector<HTMLButtonElement>('.row')!.click(); await settle()
    expect(document.body.textContent).toContain('检查完成')
    expect(document.querySelector('.usage')?.textContent).toContain('15')
    expect(document.querySelector('.artifacts .link')?.textContent).toBe('report.md')
    button(label('settings.title')).click(); await settle()
    expect(document.querySelectorAll('[aria-current=page]')).toHaveLength(1)
    expect(document.querySelector('[aria-current=page]')?.classList.contains('settings-entry')).toBe(true)
  })

  it('does not replace a newly selected log with an older request result', async () => {
    const logs = [deferred<{ log: string }>(), deferred<{ log: string }>()]
    let count = 0
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => method === 'plugin.history.log' ? logs[count++].promise : original(method, params))
    components.push(mount(App, { target: document.body })); flushSync(); await settle(); await settle()
    const rows = document.querySelectorAll<HTMLButtonElement>('.row')
    rows[0].click(); await settle(); rows[1].click(); await settle()
    expect(document.querySelector('.loading-status')?.textContent).toBe(label('history.loading'))
    logs[1].resolve({ log: 'second current log' }); await settle()
    logs[0].resolve({ log: 'first stale log' }); await settle()
    expect(document.querySelector('.log')?.textContent).toContain('second current log')
    expect(document.querySelector('.log')?.textContent).not.toContain('first stale log')
  })

  it('returns keyboard focus to the empty history after its last records are cleared', async () => {
    let cleared = false
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => {
      if (method === 'plugin.history.clear') { cleared = true; return Promise.resolve({}) }
      if (method === 'plugin.history.list' && cleared) return Promise.resolve({ runs: [] })
      return original(method, params)
    })
    components.push(mount(App, { target: document.body })); flushSync(); await settle(); await settle()
    const row = document.querySelector<HTMLButtonElement>('.row')!
    row.focus(); row.dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true })); await settle()
    button(label('history.clearAll')).click(); await settle()
    button(label('history.clearAll')).click(); await settle()
    await vi.waitFor(() => expect(document.activeElement?.textContent).toBe(label('history.empty')))
    expect(document.querySelectorAll('.row')).toHaveLength(0)
  })

  it('renders stream tools and detailed history with readable non-color status text', async () => {
    components.push(mount(RunStream, { target: document.body, props: { items: [{ type: 'tool', name: 'read', brief: 'projects/demo.md' }, { type: 'text', text: '检查中' }] } }))
    components.push(mount(RunLog, { target: document.body, props: { label, run: { ...record('error'), status: 'error', stderr_tail: '磁盘不可用' }, log: '保留的日志内容' } }))
    flushSync(); await settle()
    expect(document.querySelector('.tool')?.textContent).toContain('projects/demo.md')
    expect(document.querySelector('.status')?.textContent).toBe(label('status.error'))
    expect(document.querySelector('.stderr')?.textContent).toBe('磁盘不可用')
  })
})
