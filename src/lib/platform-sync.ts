/**
 * Synchronous platform probe — deliberately dependency-free.
 *
 * Lives apart from `platform.svelte.ts` because `editor-kit/` imports it, and
 * that bundle is size-asserted by `scripts/check-editor-kit-build.mjs`; pulling
 * in `@tauri-apps/plugin-os` for a user-agent test would be a poor trade.
 */

/**
 * True on Apple's WebKit (macOS WKWebView / iOS). Used where the answer is
 * needed before a promise can settle — editor construction hands
 * `platform.isMacOS` to moraya-core synchronously.
 *
 * That flag gates WKWebView-only workarounds (caret decoration on empty
 * paragraphs, empty-doc focus recovery) and NOT the keymap, which
 * prosemirror-keymap resolves on its own. Both call sites used to pass a
 * hardcoded `true`, leaving those workarounds running under WebView2 on
 * Windows — where the focus-recovery branch can steal focus after a document
 * is cleared.
 *
 * Mirrors `defaultPlatform()` in moraya-core's `setup.ts`.
 */
export function isApplePlatformSync(): boolean {
  if (typeof navigator === 'undefined') return false
  // `navigator.platform` is deprecated but remains the most direct signal in
  // WKWebView; the user agent is the fallback and carries the same words.
  const legacy = (navigator as { platform?: string }).platform ?? ''
  const ua = navigator.userAgent ?? ''
  return /Mac|iPhone|iPad|iPod/i.test(legacy) || /Macintosh|iPhone|iPad|iPod/i.test(ua)
}
