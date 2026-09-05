export interface ModalFocusOptions {
  onClose: () => void
  canClose?: () => boolean
}

const modals: HTMLElement[] = []
const focusableSelector = 'button, input:not([type="hidden"]), select, textarea, a[href], summary, [tabindex], [contenteditable="true"]'

function focusable(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(focusableSelector)).filter((element) => {
    if (element.tabIndex < 0 || element.matches(':disabled') || element.closest('[hidden], [inert], [aria-hidden="true"]')) return false
    for (let parent: HTMLElement | null = element; parent && node.contains(parent); parent = parent.parentElement) {
      const style = node.ownerDocument.defaultView?.getComputedStyle(parent)
      if (style?.display === 'none' || style?.visibility === 'hidden') return false
    }
    return true
  })
}

/** Lightweight modal action. Domain-specific close/save rules stay with its caller. */
export function modalFocus(node: HTMLElement, initialOptions: ModalFocusOptions) {
  let options = initialOptions
  const document = node.ownerDocument
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const oldTabindex = node.getAttribute('tabindex')
  if (oldTabindex === null) node.tabIndex = -1
  modals.push(node)
  const isTop = () => modals.filter((modal) => modal.isConnected).at(-1) === node
  const enter = () => {
    const targets = focusable(node)
    ;(targets.find((target) => target.hasAttribute('data-initial-focus')) ?? targets[0] ?? node).focus({ preventScroll: true })
  }
  queueMicrotask(() => { if (node.isConnected && isTop()) enter() })

  const keydown = (event: KeyboardEvent) => {
    if (!isTop() || event.defaultPrevented || event.isComposing || event.keyCode === 229) return
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      if (options.canClose?.() !== false) options.onClose()
      return
    }
    if (event.key !== 'Tab') return
    const targets = focusable(node)
    const first = targets[0], last = targets.at(-1)
    const current = document.activeElement
    if (!first || !node.contains(current) || current === node || (event.shiftKey ? current === first : current === last)) {
      event.preventDefault()
      ;(event.shiftKey ? last ?? node : first ?? node).focus()
    }
  }
  const focusin = (event: FocusEvent) => {
    if (isTop() && event.target instanceof Node && !node.contains(event.target)) enter()
  }
  document.addEventListener('keydown', keydown)
  document.addEventListener('focusin', focusin)

  return {
    update(next: ModalFocusOptions) { options = next },
    destroy() {
      document.removeEventListener('keydown', keydown)
      document.removeEventListener('focusin', focusin)
      const index = modals.indexOf(node)
      if (index >= 0) modals.splice(index, 1)
      if (oldTabindex === null) node.removeAttribute('tabindex')
      else node.setAttribute('tabindex', oldTabindex)
      queueMicrotask(() => {
        const top = modals.filter((modal) => modal.isConnected).at(-1)
        if (previous?.isConnected && (!top || top.contains(previous))) previous.focus({ preventScroll: true })
        else if (top) (focusable(top)[0] ?? top).focus({ preventScroll: true })
      })
    },
  }
}
