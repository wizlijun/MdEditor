/**
 * Mermaid 11.15+ builds its sanitized classDef/theme CSS through
 * `new CSSStyleSheet()`. Older WKWebViews expose CSSStyleSheet instances on
 * `<style>.sheet` but do not allow constructing one directly. Keep those
 * supported systems on the same safe CSSOM path without adding styles to the
 * live document.
 */
export function ensureMermaidCssStyleSheetCompatibility(): void {
  if (typeof document === 'undefined') return

  const NativeStyleSheet = globalThis.CSSStyleSheet
  if (typeof NativeStyleSheet === 'function') {
    try {
      const sheet = new NativeStyleSheet()
      sheet.insertRule(':root {}', 0)
      return
    } catch {
      // Safari/WKWebView exposed CSSStyleSheet before it became constructable.
    }
  }

  const LegacyStyleSheet = function CSSStyleSheet() {
    const style = document.createElement('style')
    ;(document.head ?? document.documentElement).appendChild(style)
    const sheet = style.sheet
    style.remove()
    if (!sheet) throw new Error('CSSStyleSheet compatibility setup failed')
    return sheet
  } as unknown as typeof CSSStyleSheet

  Object.defineProperty(globalThis, 'CSSStyleSheet', {
    configurable: true,
    writable: true,
    value: LegacyStyleSheet,
  })
}
