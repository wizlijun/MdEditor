// inbox.ts — the report inbox's data layer: which files count as reports, in
// what order, where a row's title comes from, and what a delete takes along.
//
// Reports are AGENT products (✦) that this window can edit in place. Unlike
// idea-spark's inbox there is no rename or inbox-owned dirty state: a report
// either exists or it doesn't, and deletion is the data layer's only mutation.
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
  /** Report file name inside the trace directory (whether or not it exists yet). */
  name: string
  /** Frontmatter `title` (report's, or the request's for an orphan), or null. */
  title: string | null
  /** False for a delegation whose report never landed — the request directory
   *  exists but `<name>` doesn't. Those rows are how a lost/failed/still-running
   *  task stays VISIBLE instead of silently vanishing on refresh. */
  hasReport: boolean
}

/** The document represented by an inbox row. Finished rows edit the report;
 * unfinished rows edit the only durable artifact they have: the request. */
export function documentPathFor(dir: string, report: Pick<ReportEntry, 'name' | 'hasReport'>): string {
  return report.hasReport ? `${dir}/${report.name}` : `${dir}/${requestPathFor(report.name)}`
}

/** `2026-08-18-143012-source-trace.md` → `2026-08-18-143012-source-trace/00-request.md`.
 *  The request lives INSIDE the materials directory (number 00, materials start
 *  at 01) so a delete takes it along and the report can link it relatively. */
export function requestPathFor(reportName: string): string {
  return `${materialsDirFor(reportName)}/00-request.md`
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
 * Lists the trace directory's rows, newest first (the timestamp names sort
 * lexically, so reverse name order IS reverse time order). Two kinds:
 *   - report files (`*-source-trace.md`) — finished traces;
 *   - ORPHANS: a `*-source-trace/` directory with no matching report file.
 *     That is a delegation that hasn't (or never) produced its report — still
 *     running, failed, or aborted before the run even started. Listing them is
 *     what keeps a task visible across a window refresh.
 * Throws when the directory itself can't be listed — the caller shows
 * "couldn't read" instead of a lying "no traces yet". A single row whose
 * *content* can't be read still lists, with a null title.
 */
export async function listReports(io: Pick<InboxIo, 'list' | 'read'>, dir: string): Promise<ReportEntry[]> {
  const { entries } = await io.list(dir)
  const reportNames = new Set(
    entries.filter((e) => !e.is_dir && e.name.endsWith(REPORT_SUFFIX)).map((e) => e.name),
  )
  const orphanNames = entries
    .filter((e) => e.is_dir && e.name.endsWith('-source-trace') && !reportNames.has(`${e.name}.md`))
    .map((e) => `${e.name}.md`)
  const names = [...reportNames, ...orphanNames].sort().reverse()
  return Promise.all(
    names.map(async (name, i): Promise<ReportEntry> => {
      const hasReport = reportNames.has(name)
      if (i >= TITLE_READS) return { name, title: null, hasReport }
      const titlePath = documentPathFor(dir, { name, hasReport })
      try {
        return { name, title: titleOf((await io.read(titlePath)).content), hasReport }
      } catch {
        return { name, title: null, hasReport }
      }
    }),
  )
}

/** How long a derived request title may run before it is cut. */
const TITLE_MAX = 60

/**
 * The request document written to `requestPathFor(...)` at delegation time —
 * the user's own words, saved BEFORE the agent is involved so nothing about
 * the ask is lost to a crash, a failed run, or a closed window.
 *
 * OKF: `type: Trace Request` (registered host-side in concept.ts; Human tier
 * in searchidx origin mapping). Human-authored, so no `generated` stamp.
 */
export function buildRequestDoc(text: string): string {
  const firstLine =
    text
      .split('\n')
      .map((l) => l.replace(/^>\s*/, '').trim())
      .find((l) => l !== '') ?? ''
  const cut = firstLine.length > TITLE_MAX ? `${firstLine.slice(0, TITLE_MAX)}…` : firstLine
  const title = cut || 'Trace request'
  // YAML scalar safety: quote anything that could open a flow/keyed construct.
  const needsQuote = /^[\s:>#\-?&*!|%@`"'{[\]}]|[:#]\s|\t/.test(title)
  const yamlTitle = needsQuote ? `"${title.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"` : title
  return `---\ntype: Trace Request\ntitle: ${yamlTitle}\n---\n\n${text.replace(/\s+$/, '')}\n`
}

/** Undoes `buildRequestDoc`'s wrapper for loading a request back into the
 *  composer. Content without a frontmatter block passes through untouched. */
export function stripFrontmatter(content: string): string {
  const m = /^---\r?\n[\s\S]*?\r?\n---\r?\n?/.exec(content)
  return m ? content.slice(m[0].length).replace(/^\r?\n/, '') : content
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
