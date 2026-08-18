// inbox.ts — the report inbox's data layer: which files count as reports, in
// what order, where a row's title comes from, and what a delete takes along.
//
// Reports are AGENT products (✦), not documents this window edits — so unlike
// idea-spark's inbox there is no rename, no dirty state, no status machine: a
// report either exists or it doesn't, and the only mutations are delete.
//
// No `$state` and no bridge import: pure logic over injected IO, so the tests
// need nothing but plain objects.

/** The naming convention that IS the report's identity: the delegate call
 *  names the output `<YYYY-MM-DD>-<HHmmss>-source-trace.md`, and the task
 *  template's write permissions are scoped to this very pattern. */
export const REPORT_SUFFIX = '-source-trace.md'

export interface InboxIo {
  list(path: string): Promise<{ entries: Array<{ name: string; is_dir: boolean }> }>
  read(path: string): Promise<{ content: string }>
  remove(path: string): Promise<{ ok: true }>
}

export interface ReportEntry {
  /** File name inside the trace directory. */
  name: string
  /** Frontmatter `title`, or null when unreadable/absent (row shows the name). */
  title: string | null
}

/** How many rows get their title read. Beyond this the rows still list — they
 *  just show file names. (An inbox of hundreds of traces is not the case this
 *  window optimizes for; the cap keeps a huge directory from costing hundreds
 *  of reads every refresh.) */
const TITLE_READS = 100

/** `2026-08-18-143012-source-trace.md` → `2026-08-18-143012-source-trace`. */
export function materialsDirFor(reportName: string): string {
  return reportName.replace(/\.md$/, '')
}

/** Parses the creation moment out of the report's timestamp name; null for
 *  anything that was never a real timestamp (round-trip checked). */
export function createdFromName(name: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})-(\d{2})(\d{2})(\d{2})-source-trace\.md$/.exec(name)
  if (!m) return null
  const [y, mo, d, h, mi, s] = [m[1], m[2], m[3], m[4], m[5], m[6]].map(Number)
  const date = new Date(y, mo - 1, d, h, mi, s)
  const same =
    date.getFullYear() === y &&
    date.getMonth() === mo - 1 &&
    date.getDate() === d &&
    date.getHours() === h &&
    date.getMinutes() === mi &&
    date.getSeconds() === s
  return same ? date : null
}

/** Coarse "(value, unit)" pair for `Intl.RelativeTimeFormat` — negative,
 *  i.e. in the past. Same tiers as idea-spark's inbox. */
export function relativeAge(
  from: Date,
  now: Date,
): { value: number; unit: 'minute' | 'hour' | 'day' | 'month' | 'year' } {
  const past = (n: number) => (n === 0 ? 0 : -n)
  const minutes = Math.max(0, Math.floor((now.getTime() - from.getTime()) / 60_000))
  if (minutes < 60) return { value: past(minutes), unit: 'minute' }
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return { value: past(hours), unit: 'hour' }
  const days = Math.floor(hours / 24)
  if (days < 30) return { value: past(days), unit: 'day' }
  const months = Math.floor(days / 30)
  if (months < 12) return { value: past(months), unit: 'month' }
  return { value: past(Math.floor(days / 365)), unit: 'year' }
}

/** Frontmatter `title:` of a report body, or null. Tolerant by hand rather
 *  than via a YAML dependency: a title is one scalar line, and a report whose
 *  frontmatter we can't read is still a listable report. */
function titleOf(content: string): string | null {
  const m = /^---\r?\n([\s\S]*?)\r?\n---/.exec(content)
  if (!m) return null
  const line = m[1].split(/\r?\n/).find((l) => /^title\s*:/.test(l))
  if (!line) return null
  const raw = line.replace(/^title\s*:/, '').trim()
  const unquoted = /^(["']).*\1$/.test(raw) ? raw.slice(1, -1) : raw
  return unquoted || null
}

/**
 * Lists the trace directory's reports, newest first (the timestamp names sort
 * lexically, so reverse name order IS reverse time order). Throws when the
 * directory itself can't be listed — the caller shows "couldn't read" instead
 * of a lying "no traces yet". A single report whose *content* can't be read
 * still lists, with a null title.
 */
export async function listReports(io: Pick<InboxIo, 'list' | 'read'>, dir: string): Promise<ReportEntry[]> {
  const { entries } = await io.list(dir)
  const names = entries
    .filter((e) => !e.is_dir && e.name.endsWith(REPORT_SUFFIX))
    .map((e) => e.name)
    .sort()
    .reverse()
  return Promise.all(
    names.map(async (name, i): Promise<ReportEntry> => {
      if (i >= TITLE_READS) return { name, title: null }
      try {
        return { name, title: titleOf((await io.read(`${dir}/${name}`)).content) }
      } catch {
        return { name, title: null }
      }
    }),
  )
}

/**
 * Everything deleting `name` takes with it: the report, then each file in its
 * materials directory. The host's `vault.remove` refuses directories, so the
 * materials are removed one file at a time — the emptied directory entry
 * itself stays behind, which is invisible everywhere this vault is read.
 */
export async function previewDelete(
  io: Pick<InboxIo, 'list'>,
  dir: string,
  name: string,
): Promise<string[]> {
  const lines = [`${dir}/${name}`]
  const matDir = `${dir}/${materialsDirFor(name)}`
  try {
    const { entries } = await io.list(matDir)
    for (const e of entries) if (!e.is_dir) lines.push(`${matDir}/${e.name}`)
  } catch {
    // No materials directory — the report is all there is.
  }
  return lines
}

/** Report first: a partial failure leaves orphaned materials (recoverable and
 *  visible as such), not a report whose links point at deleted files. */
export async function deleteReport(io: InboxIo, dir: string, name: string): Promise<void> {
  for (const path of await previewDelete(io, dir, name)) {
    await io.remove(path)
  }
}
