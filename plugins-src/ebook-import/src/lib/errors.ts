// src/lib/errors.ts — classifies the backend's own English error strings
// (plugins-src/ebook-import/backend/src) into a MessageKey so the UI can show
// a localized, actionable sentence instead of raw English. Safe to match on
// exact backend wording: both sides live in this repo/version, and
// strings.test.ts + errors.test.ts pin the mapping so a backend wording
// change that breaks it fails CI instead of silently falling back to English.
//
// Deliberately narrow: only backend strings this plugin's own pipeline emits
// are mapped. Anything else (host RPC errors, unexpected panics, a future
// backend string nobody's taught this classifier yet) returns `null` and the
// UI falls back to showing the raw message — never a wrong translation.

import { t, type MessageKey } from './strings'

/** One (matcher, key) rule, tried in order; the first match wins. */
const RULES: Array<{ test: (msg: string) => boolean; key: MessageKey }> = [
  { test: (m) => /^no vault configured$/i.test(m), key: 'err.noVault' },
  { test: (m) => /^calibre not found$/i.test(m), key: 'err.calibreMissing' },
  { test: (m) => /^ebook-convert timed out after\b/i.test(m), key: 'err.calibreTimeout' },
  {
    test: (m) => /^ebook-convert exited with status\b/i.test(m) || /^failed to launch ebook-convert:/i.test(m),
    key: 'err.calibreFailed',
  },
  { test: (m) => /^ebooks_root must be a vault-relative path$/i.test(m), key: 'err.badRoot' },
  { test: (m) => /^could not derive a directory name for this book$/i.test(m), key: 'err.noTitle' },
  { test: (m) => /^OCR only supports PDF input\b/i.test(m), key: 'err.ocrOnlyPdf' },
  { test: (m) => /^OCR produced no content for any of\b/i.test(m), key: 'err.ocrEmpty' },
  { test: (m) => /^ocr service unreachable:/i.test(m), key: 'err.ocrUnreachable' },
  {
    test: (m) => /^Baidu OCR API error\b/i.test(m) || /^baidu ocr: task\b/i.test(m),
    key: 'err.baiduFailed',
  },
  { test: (m) => /^unsupported file extension\b/i.test(m), key: 'err.unsupportedType' },
]

/** Classifies a raw backend error string, or `null` if it's not one we know. */
export function errorKey(msg: string): MessageKey | null {
  const rule = RULES.find((r) => r.test(msg))
  return rule ? rule.key : null
}

/**
 * Backend strings whose fixed template carries no dynamic content beyond the
 * matched phrase itself — showing the raw message alongside the localized
 * sentence would just repeat the same (English) words, so `detail` is
 * omitted for these.
 */
const NO_EXTRA_INFO = new Set<MessageKey>([
  'err.noVault',
  'err.calibreMissing',
  'err.badRoot',
  'err.noTitle',
])

/**
 * Resolves a raw backend error message to what the UI should show: a
 * localized sentence (falling back to the raw message when unmatched) plus
 * an optional `detail` carrying the original technical text when the match
 * embeds extra information (a path, a status code, a timeout, …) that the
 * localized sentence doesn't repeat.
 */
export function describeError(msg: string): { text: string; detail?: string } {
  const key = errorKey(msg)
  if (!key) return { text: msg }
  const text = t(key)
  return NO_EXTRA_INFO.has(key) ? { text } : { text, detail: msg }
}
