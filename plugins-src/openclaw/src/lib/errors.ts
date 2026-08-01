// src/lib/errors.ts — classifies the backend's own English error strings
// (plugins-src/openclaw/backend/src/{lib,pair}.rs) into a localized sentence
// so the UI shows something readable instead of raw English or a bare HTTP
// status. Same approach as plugins-src/ebook-import/src/lib/errors.ts.
//
// Deliberately narrow: only backend strings this plugin's own protocol emits
// are mapped. Anything else (host RPC errors, unexpected panics, a future
// backend string nobody's taught this classifier yet) is shown as-is —
// never a wrong translation.

import { t, type MessageKey } from './strings'

/** One (matcher, key) rule, tried in order; the first match wins. */
const RULES: Array<{ test: (msg: string) => boolean; key: MessageKey }> = [
  { test: (m) => /^access token not configured\b/i.test(m), key: 'err.noAccessToken' },
  { test: (m) => /^no relay URL$/i.test(m), key: 'err.noRelayUrl' },
]

/** Classifies a raw backend error string, or `null` if it's not one we know. */
export function errorKey(msg: string): MessageKey | null {
  const rule = RULES.find((r) => r.test(msg))
  return rule ? rule.key : null
}

/**
 * Resolves a raw backend error message (from `connect`, `pair_create`,
 * `pair_claim`, …) to what the UI should show: a localized sentence, falling
 * back to the raw message when unmatched.
 */
export function describeError(msg: string): string {
  // pair.rs relays the pairing server's HTTP status verbatim ("status 404")
  // — the only backend error here with a dynamic part.
  const status = msg.match(/^status (\d+)$/)
  if (status) return t('err.pairingFailed', { status: status[1] })
  const key = errorKey(msg)
  return key ? t(key) : msg
}
