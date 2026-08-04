import YAML from 'yaml'
import { normalizeRecordConfidence, starOf, type OpenDecision, type ArchivedDecision } from './model'

/** Human-readable confidence for note bodies: stars + anchor percent. */
function confLabel(c: number): string {
  return `${'★'.repeat(starOf(c))} ≈${Math.round(c * 100)}%`
}

const FM = /^---\n([\s\S]*?)\n---\n?/

function buildNote(frontmatter: object, bodyLines: string[]): string {
  const fm = YAML.stringify(frontmatter).trimEnd()
  return `---\n${fm}\n---\n\n${bodyLines.join('\n')}\n`
}
function readFrontmatter(md: string): any {
  const m = md.match(FM)
  if (!m) return null
  try { return YAML.parse(m[1]) } catch { return null }
}

export function serializeBoard(decisions: OpenDecision[]): string {
  const body = ['# 未决决策', '']
  for (const d of decisions) {
    body.push(`## ${d.title}`)
    body.push(`- 预测:${d.prediction}(信心 ${confLabel(d.confidence)})· 检查 ${d['check-date']}`)
    body.push('')
  }
  return buildNote({ type: 'Decision Board', decisions }, body)
}
export function parseBoard(md: string): OpenDecision[] {
  const fm = readFrontmatter(md)
  const arr = Array.isArray(fm?.decisions) ? (fm.decisions as OpenDecision[]) : []
  return arr.map((d) => normalizeRecordConfidence(d))
}
export function serializeArchive(resolved: string, decisions: ArchivedDecision[]): string {
  const lines = [`# ${resolved} 裁决`, '']
  for (const d of decisions) {
    const mark = d.status === 'closed' ? (d.outcome === 'hit' ? '✅' : d.outcome === 'miss' ? '❌' : '◐') : d.status === 'dropped' ? '⊘' : '⬇'
    lines.push(`## ${d.id} — ${d.status} ${mark}`)
    lines.push(`- 预测:${d.prediction}(信心 ${confLabel(d.confidence)})`)
    lines.push('')
  }
  return buildNote({ type: 'Decision Archive', resolved, decisions }, lines)
}
export function parseArchive(md: string): ArchivedDecision[] {
  const fm = readFrontmatter(md)
  const arr = Array.isArray(fm?.decisions) ? (fm.decisions as ArchivedDecision[]) : []
  return arr.map((d) => normalizeRecordConfidence(d))
}
