// Run times are stamped in UTC by the backend (`chrono::Utc::now()`); the UI
// renders them in the user's LOCAL timezone. `new Date(iso)` parses the instant
// and the get*() accessors below all read local-time components, so a run at
// 08:42 local (00:42 UTC) shows as "08:42" — never the raw UTC clock.

const pad = (n: number) => String(n).padStart(2, '0')

/** "07-31 08:42" in local time. Falls back to the raw string on a bad date. */
export function fmtShort(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** "2026-07-31 08:42:33" in local time. Falls back to the raw string on a bad date. */
export function fmtFull(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}
