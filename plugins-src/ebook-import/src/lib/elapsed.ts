// src/lib/elapsed.ts — how long an AI read has been going. Shared by the import
// queue's rows and the library's, which show the same "AI 阅读中… 3m12s" clock.

/**
 * `startedAt` (an ISO 8601 instant from the backend) counted up to `nowMs`,
 * as `0s` / `3m12s` / `3h7m`. Empty when nothing has started or the timestamp
 * won't parse — the caller renders nothing rather than "NaNs".
 */
export function formatElapsed(startedAt: string | undefined, nowMs: number): string {
  if (!startedAt) return ''
  const began = Date.parse(startedAt)
  if (Number.isNaN(began)) return ''
  // Clamped at zero: the start time is the backend's clock and `nowMs` is the
  // window's, and a run must never read as having started in the future.
  const s = Math.max(0, Math.floor((nowMs - began) / 1000))
  const m = Math.floor(s / 60)
  if (m < 60) return m > 0 ? `${m}m${s % 60}s` : `${s}s`
  return `${Math.floor(m / 60)}h${m % 60}m`
}
