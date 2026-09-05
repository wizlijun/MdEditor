// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { modalFocus } from './modal-focus'

const actions: ReturnType<typeof modalFocus>[] = []
function dialog() {
  const node = document.createElement('div')
  node.setAttribute('role', 'dialog')
  node.innerHTML = '<button>First</button><input aria-label="Field"><button>Last</button>'
  document.body.append(node)
  return node
}
function use(node: HTMLElement, options: Parameters<typeof modalFocus>[1]) {
  const action = modalFocus(node, options)
  actions.push(action)
  return action
}
function key(key: string, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options })
  document.activeElement!.dispatchEvent(event)
  return event
}
afterEach(async () => {
  actions.splice(0).reverse().forEach((action) => action.destroy())
  await Promise.resolve()
  document.body.innerHTML = ''
})

describe('modalFocus', () => {
  it('focuses the initial action, traps both tab boundaries, and restores the trigger', async () => {
    const trigger = document.createElement('button')
    document.body.append(trigger)
    trigger.focus()
    const node = dialog(), buttons = node.querySelectorAll('button')
    const action = use(node, { onClose: vi.fn() })
    await Promise.resolve()
    expect(document.activeElement).toBe(buttons[0])
    expect(key('Tab', { shiftKey: true }).defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(buttons[1])
    expect(key('Tab').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(buttons[0])
    action.destroy()
    actions.pop()
    node.remove()
    await Promise.resolve()
    expect(document.activeElement).toBe(trigger)
  })

  it('ignores disabled and hidden controls and honours data-initial-focus', async () => {
    const node = dialog()
    node.querySelector('button')!.disabled = true
    node.querySelector('input')!.style.display = 'none'
    node.querySelectorAll('button')[1].setAttribute('data-initial-focus', '')
    use(node, { onClose: vi.fn() })
    await Promise.resolve()
    expect(document.activeElement).toBe(node.lastChild)
    expect(key('Tab').defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(node.lastChild)
  })

  it('only closes/traps the nested modal and returns to its parent trigger', async () => {
    const parent = dialog(), parentClose = vi.fn(), childClose = vi.fn()
    use(parent, { onClose: parentClose })
    await Promise.resolve()
    const trigger = parent.querySelector('input')!
    trigger.focus()
    const child = dialog(), childAction = use(child, { onClose: childClose })
    await Promise.resolve()
    key('Escape')
    expect(childClose).toHaveBeenCalledOnce()
    expect(parentClose).not.toHaveBeenCalled()
    childAction.destroy()
    actions.pop()
    child.remove()
    await Promise.resolve()
    expect(document.activeElement).toBe(trigger)
    key('Escape')
    expect(parentClose).toHaveBeenCalledOnce()
  })

  it('respects saving, updates close options, and does not close during IME', async () => {
    const close = vi.fn(), nextClose = vi.fn()
    const action = use(dialog(), { onClose: close, canClose: () => false })
    await Promise.resolve()
    expect(key('Escape').defaultPrevented).toBe(true)
    expect(close).not.toHaveBeenCalled()
    action.update({ onClose: nextClose })
    key('Escape', { isComposing: true })
    expect(nextClose).not.toHaveBeenCalled()
    key('Escape')
    expect(nextClose).toHaveBeenCalledOnce()
  })

  it('does not intercept shortcut recording or ordinary editor keys', async () => {
    const close = vi.fn(), node = dialog()
    use(node, { onClose: close })
    await Promise.resolve()
    expect(key('k', { metaKey: true }).defaultPrevented).toBe(false)
    node.addEventListener('keydown', (event) => event.preventDefault())
    key('Escape')
    expect(close).not.toHaveBeenCalled()
  })

  it('keeps programmatic/background keyboard focus inside the active modal', async () => {
    const outside = document.createElement('input')
    document.body.append(outside)
    const node = dialog()
    use(node, { onClose: vi.fn() })
    await Promise.resolve()
    outside.focus()
    expect(node.contains(document.activeElement)).toBe(true)
  })

  it('keeps an empty dialog focusable without adding it to the normal tab order', async () => {
    const node = dialog()
    node.innerHTML = '<button disabled>Working</button>'
    use(node, { onClose: vi.fn() })
    await Promise.resolve()
    expect(document.activeElement).toBe(node)
    expect(node.tabIndex).toBe(-1)
    expect(key('Tab').defaultPrevented).toBe(true)
  })
})
