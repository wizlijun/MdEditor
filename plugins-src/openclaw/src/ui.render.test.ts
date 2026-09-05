import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import App from './App.svelte'
import Composer from './components/chat/Composer.svelte'
import RemoteOnboarding from './components/chat/RemoteOnboarding.svelte'
import PairingDialog from './components/chat/PairingDialog.svelte'
import PendingClaimToast from './components/chat/PendingClaimToast.svelte'
import MessageBubble from './components/chat/MessageBubble.svelte'
import SessionPicker from './components/chat/SessionPicker.svelte'
import { setLocale, t } from './lib/strings'
import { state as clientState } from './lib/openclaw/client.svelte'
import { startBridge } from './lib/openclaw/commands'

const components: ReturnType<typeof mount>[] = []
const listeners: ((payload: unknown) => void)[] = []
function emit(payload: unknown) { listeners.forEach((listener) => listener(payload)) }
async function settle() { await Promise.resolve(); await Promise.resolve(); await tick(); flushSync() }
function button(text: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text)! }
beforeEach(() => {
  setLocale('zh')
  clientState.currentSessionId = 'session-1'
  clientState.sessions = [{ id: 'session-1', title: '项目讨论' }]
  clientState.messagesBySession = {}
  clientState.status = 'connected'
  clientState.error = null
  window.notemd = {
    pluginId: 'notemd.openclaw-chat', locale: 'zh', theme: 'system',
    onMessage: (listener) => { if (!listeners.includes(listener)) listeners.push(listener) },
    request: vi.fn(async (method: string) => {
      if (method === 'plugin.connect') return 'host'
      if (method === 'plugin.pair_create') return { code: 'abc-def-012-345-678-9ab', pairing_id: 'pair-1', expires_at: Date.now() + 120000, qr_svg: '<svg viewBox="0 0 20 20"><rect width="20" height="20" /></svg>' }
      if (method === 'plugin.list_devices') return []
      return {}
    }),
  }
})
afterEach(async () => { for (const c of components.splice(0)) await unmount(c); document.body.innerHTML = ''; vi.restoreAllMocks() })

describe('OpenClaw accessible window UI', () => {
  it('retains the same draft through reconnection and a required pairing screen', async () => {
    let connections = 0
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => {
      if (method === 'plugin.connect' && ++connections === 2) return Promise.reject(new Error('not paired'))
      return original(method, params)
    })
    components.push(mount(App, { target: document.body })); flushSync(); await settle(); await settle()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '连接恢复后继续发送'; textarea.dispatchEvent(new Event('input', { bubbles: true })); flushSync()
    clientState.error = 'offline'; flushSync()
    button(t('chat.retry')).click()
    await vi.waitFor(() => expect(document.querySelector('.onboard')).not.toBeNull())
    const code = document.querySelector<HTMLInputElement>('.onboard input')!
    code.value = 'abc-def-012-345-678-9ab'; code.dispatchEvent(new Event('input', { bubbles: true })); flushSync()
    document.querySelector('.onboard')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    await vi.waitFor(() => expect(document.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe('连接恢复后继续发送'))
  })

  it.each([false, true])('restores composer focus after async send without stealing it (%s)', async (moved) => {
    let complete!: () => void
    window.notemd.request = vi.fn(() => new Promise<void>((resolve) => { complete = resolve }))
    components.push(mount(Composer, { target: document.body })); flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '继续对话'; textarea.dispatchEvent(new Event('input', { bubbles: true })); flushSync()
    const other = document.createElement('button'); document.body.append(other)
    textarea.focus()
    document.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); flushSync()
    textarea.blur()
    if (moved) other.focus()
    complete(); await settle()
    await vi.waitFor(() => expect(document.activeElement).toBe(moved ? other : textarea))
  })

  it('renders the real chat window with named session and composer controls', async () => {
    components.push(mount(App, { target: document.body })); flushSync(); await settle(); await settle()
    expect(document.querySelector('main.ui-surface')).not.toBeNull()
    expect(document.querySelector('select')?.getAttribute('aria-label')).toBe(t('chat.session'))
    expect(document.querySelector('textarea')?.getAttribute('aria-label')).toBe(t('chat.typeToOpenClaw'))
    expect(document.querySelector('[role=status]')?.textContent).toBe(t('chat.status.connected'))
    expect(button(t('chat.send'))).toBeTruthy()
  })

  it('keeps unsent text and announces a send failure without clearing the draft', async () => {
    window.notemd.request = vi.fn(async () => { throw new Error('connection lost') })
    components.push(mount(Composer, { target: document.body })); flushSync()
    const textarea = document.querySelector<HTMLTextAreaElement>('textarea')!
    textarea.value = '请检查项目'; textarea.dispatchEvent(new Event('input', { bubbles: true })); flushSync()
    textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, isComposing: true, bubbles: true }))
    expect(window.notemd.request).not.toHaveBeenCalled()
    const send = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })
    textarea.dispatchEvent(send); await settle()
    expect(send.defaultPrevented).toBe(true)
    expect(textarea.value).toBe('请检查项目')
    await vi.waitFor(() => expect(textarea.disabled).toBe(false))
    expect(document.querySelector('[role=alert]')?.textContent).toContain(t('chat.sendFailed'))
  })

  it('lets keyboard users invoke attachment selection and presents upload failures', async () => {
    window.notemd.request = vi.fn(async () => { throw new Error('upload rejected') })
    components.push(mount(Composer, { target: document.body })); flushSync()
    const input = document.querySelector<HTMLInputElement>('input[type=file]')!
    const click = vi.spyOn(input, 'click')
    const attach = document.querySelector<HTMLButtonElement>('button[aria-label]')!
    attach.focus(); attach.click()
    expect(document.activeElement).toBe(attach)
    expect(click).toHaveBeenCalledOnce()
    const file = new File(['data'], 'note.txt')
    Object.defineProperty(file, 'arrayBuffer', { value: async () => new ArrayBuffer(4) })
    Object.defineProperty(input, 'files', { value: [file] })
    input.dispatchEvent(new Event('change', { bubbles: true })); await settle(); await settle()
    expect(document.querySelector('[role=alert]')?.textContent).toContain(t('chat.uploadFailed'))
  })

  it('submits pairing from a named form and keeps entered values on failure', async () => {
    const onComplete = vi.fn()
    window.notemd.request = vi.fn(async () => { throw new Error('pairing failed') })
    components.push(mount(RemoteOnboarding, { target: document.body, props: { onComplete } })); flushSync()
    const [code, hostname] = document.querySelectorAll<HTMLInputElement>('input')
    code.value = 'abc-def-012-345-678-9ab'; code.dispatchEvent(new Event('input', { bubbles: true }))
    hostname.value = '我的设备'; hostname.dispatchEvent(new Event('input', { bubbles: true })); flushSync()
    expect(code.closest('label')?.textContent).toContain(t('chat.pairingCode'))
    expect(code.getAttribute('aria-describedby')).toBe('pairing-hint')
    document.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); await settle()
    expect(document.querySelector('[role=alert]')).not.toBeNull()
    expect(code.value).toBe('abc-def-012-345-678-9ab')
    expect(onComplete).not.toHaveBeenCalled()
  })

  it('traps pairing dialog focus, handles Escape and restores the trigger on close', async () => {
    const trigger = document.createElement('button'); trigger.textContent = '打开配对'; document.body.append(trigger); trigger.focus()
    const onClose = vi.fn()
    const component = mount(PairingDialog, { target: document.body, props: { onClose } }); components.push(component)
    flushSync(); await settle()
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    expect(dialog.contains(document.activeElement)).toBe(true)
    trigger.focus()
    expect(dialog.contains(document.activeElement)).toBe(true)
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }))
    expect(dialog.contains(document.activeElement)).toBe(true)
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(onClose).toHaveBeenCalledOnce()
    components.pop(); await unmount(component); await settle()
    expect(document.activeElement).toBe(trigger)
  })

  it('stacks multiple device requests and retains failed decisions with an error', async () => {
    startBridge()
    const original = window.notemd.request
    window.notemd.request = vi.fn((method, params) => method === 'plugin.approve_pending' ? Promise.reject(new Error('not connected')) : original(method, params))
    components.push(mount(PendingClaimToast, { target: document.body })); flushSync()
    emit({ kind: 'pending-claim', data: { device_id: 'device-1', hostname: '新设备一', at: 1 } })
    emit({ kind: 'pending-claim', data: { device_id: 'device-2', hostname: '新设备二', at: 2 } }); await settle()
    expect(document.querySelectorAll('.claims .toast')).toHaveLength(2)
    button(t('chat.allow')).click(); await settle()
    expect(document.querySelectorAll('.claims .toast')).toHaveLength(2)
    expect(document.querySelector('[role=alert]')?.textContent).toContain('not connected')
  })

  it('shows failed conversation actions locally', async () => {
    window.notemd.request = vi.fn(async () => { throw new Error('new session failed') })
    components.push(mount(SessionPicker, { target: document.body })); flushSync()
    button(t('chat.newSession')).click(); await settle()
    await vi.waitFor(() => expect(document.querySelector('[role=alert]')?.textContent).toContain('new session failed'))
    expect(button(t('chat.newSession')).disabled).toBe(false)
  })

  it('renders message links as native focusable anchors without attribute injection', async () => {
    components.push(mount(MessageBubble, { target: document.body, props: { message: { id: 'message-1', role: 'agent', text: '[文档](notes/a.md" data-attacked="yes)', streaming: false } } }))
    flushSync()
    const link = document.querySelector<HTMLAnchorElement>('a')!
    expect(link.hasAttribute('data-attacked')).toBe(false)
    expect(link.tabIndex).toBe(0)
    link.focus()
    expect(document.activeElement).toBe(link)
    window.notemd.request = vi.fn(async () => { throw new Error('file unavailable') })
    link.click(); await settle()
    expect(document.querySelector('[role=alert]')?.textContent).toContain('file unavailable')
  })
})
