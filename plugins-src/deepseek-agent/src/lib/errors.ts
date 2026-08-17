// src/lib/errors.ts — classifies the backend's own English error strings
// (plugins-src/deepseek-agent/backend/src/plugin.rs and discover.rs) into a MessageKey so the
// UI can show a localized sentence instead of raw English. Safe to match on
// exact backend wording: both sides live in this repo/version, and
// errors.test.ts pins the mapping so a backend wording change that breaks it
// fails CI instead of silently falling back to English.
//
// Deliberately narrow: only backend strings this plugin's own protocol emits
// are mapped. Anything else (host RPC errors, unexpected panics, a future
// backend string nobody's taught this classifier yet) returns `null` and the
// UI falls back to showing the raw message — never a wrong translation.

import type { MessageKey } from './strings'

/** One (matcher, key) rule, tried in order; the first match wins. */
const RULES: Array<{ test: (msg: string) => boolean; key: MessageKey }> = [
  { test: (m) => /^no vault configured$/i.test(m), key: 'err.noVault' },
  // discover::NOT_FOUND is multi-line and starts with the Chinese sentence; match
  // on the package name it always names rather than on the leading words.
  { test: (m) => /dsh-acp-demo/.test(m), key: 'err.harnessNotFound' },
  { test: (m) => /is not a valid policy/.test(m), key: 'err.badPolicy' },
  { test: (m) => /^unknown task '.*'$/.test(m), key: 'err.unknownTask' },
  { test: (m) => /^run '.*' is not running$/.test(m), key: 'err.notRunning' },
]

/** Classifies a raw backend error string, or `null` if it's not one we know. */
export function errorKey(msg: string): MessageKey | null {
  const rule = RULES.find((r) => r.test(msg))
  return rule ? rule.key : null
}
