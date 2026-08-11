/**
 * When a keystroke should actually become a query.
 *
 * Pure and separate from the panel because the rules are the whole feature:
 * a query fired at the wrong moment is not just wasted work, it is work the
 * user is then made to wait for. Three rules, in order of confidence that the
 * user has finished saying something:
 *
 *  1. Mid-composition (IME) — never. The pinyin/kana buffer is not text the
 *     user has chosen yet; searching `sousu` or a half-picked candidate is
 *     searching for something nobody typed.
 *  2. A word boundary was just typed — almost immediately. Space/punctuation
 *     is the user telling us the word is finished.
 *  3. Otherwise — only after a pause, because a prefix is not a word.
 */

/** Just typed something that ends a word: fire fast. */
export const BOUNDARY_DELAY_MS = 120
/** Mid-word: wait for the typing to stop. */
export const IDLE_DELAY_MS = 400
/**
 * How long the fast (FTS-only) answer may stand as "no matches" before the
 * expensive fallback is offered automatically. Long enough that it never
 * lands mid-typing; short enough that a user who is waiting gets it without
 * knowing about the Enter key.
 */
export const DEEP_AFTER_MS = 1200
/**
 * Ceiling for one deep scan. Measured worst case on a real 1.3M-block vault
 * is ~14s for a full miss; past a few seconds a partial answer now beats a
 * complete answer later, and the backend returns what it had with
 * `truncated`.
 */
export const DEEP_TIMEOUT_MS = 4000

/**
 * Word-enders only. Deliberately NOT `\p{P}`: `"` opens a phrase and `:`
 * opens a filter (`tag:`, `path:`), so both are mid-word characters in this
 * query language, and treating them as boundaries would fire a query for a
 * half-written `tag:` on every filter the user types.
 */
const BOUNDARY = /[\s,.;!?、，。；！？…]$/u

/**
 * Scripts where one character is already a word. `增` is a real query; `a` is
 * a typo waiting for its second letter.
 */
const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u

export type TriggerDecision =
  /** Empty input — drop the results rather than leave stale ones on screen. */
  | { kind: 'clear' }
  /** Not something to search for (yet). Leave what is on screen alone. */
  | { kind: 'hold' }
  | { kind: 'search'; delayMs: number }

export function decideTrigger(value: string, composing: boolean): TriggerDecision {
  if (composing) return { kind: 'hold' }
  if (!value.trim()) return { kind: 'clear' }
  if (BOUNDARY.test(value)) return { kind: 'search', delayMs: BOUNDARY_DELAY_MS }
  if (!looksLikeAWord(value)) return { kind: 'hold' }
  return { kind: 'search', delayMs: IDLE_DELAY_MS }
}

/**
 * A single latin letter matches so much of a vault that the answer is noise,
 * and — because FTS misses it — it is also the input most likely to reach the
 * full-table fallback. One CJK character is the opposite: a specific, common,
 * complete query.
 */
export function looksLikeAWord(value: string): boolean {
  const v = value.trim()
  if (CJK.test(v)) return true
  return v.replace(/\s+/g, '').length >= 2
}
