// delegate.ts — the one call this window exists for: hand the composed
// delegation text to `host.agent.run` against the `trace-source` task.
//
// The output path is decided HERE, not by the agent: a timestamped name under
// traces/ is appended as an `Output:` line (the template forbids renaming it),
// which is what makes `notify.expect_file` predictable — the reminder can point
// at the report before the run has written a byte. The reminder itself is the
// agent plugin's job: this window may well be closed when the run ends.
//
// No `$state` and no store access here: IO + pure mapping, testable with a
// stubbed `window.notemd`.
import { agentRun, vaultExists, vaultRead, vaultWrite } from './bridge'
import { seedTraceTemplate, TRACE_TASK_ID, type SeedIo } from './trace-template'
import { t } from './strings'

export type DelegateResult =
  | { ok: true; runId: string; outRel: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }

/** `('/V', 'traces/a.md')` → `/V/traces/a.md`, tolerating stray slashes. */
function absolute(root: string, rel: string): string {
  return `${root.replace(/\/+$/, '')}/${rel.replace(/^\/+/, '')}`
}

/** 摘要落用户设置的目录,`<时间戳>-source-trace.md` 定名——名字即身份:
 *  inbox 靠它认报告,模板写权限靠它圈定,调用方靠它预知 expect_file。 */
export function traceOutputRel(now: Date, traceDir: string): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const d = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`
  const t = `${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  return `${traceDir.replace(/\/+$/, '')}/${d}-${t}-source-trace.md`
}

const seedIo: SeedIo = {
  exists: (path) => vaultExists(path).then((r) => r.exists === true),
  read: (path) => vaultRead(path).then((r) => r.content),
  write: async (path, content) => {
    await vaultWrite(path, content)
  },
}

/**
 * Puts the task template on disk if it isn't there already, and never throws.
 * Best-effort on purpose: a read-only `.notemd/` must not cost the user a
 * delegation against a template seeded on an earlier run — the authoritative
 * complaint about a missing template is the agent's own `unknown task`.
 */
async function seed(): Promise<void> {
  try {
    await seedTraceTemplate(seedIo)
  } catch (e) {
    console.warn('[trace-source] seeding the task template failed:', e)
  }
}

/**
 * Best-effort template seeding for callers other than the delegate itself —
 * the settings popover's "edit prompt" uses it so the file it opens is the
 * one the agent will actually read, not a blank buffer.
 */
export async function seedTemplates(): Promise<void> {
  await seed()
}

/**
 * Starts one trace run. `text` is the composed delegation (quoted passage,
 * optional `Source-Doc:` line, the user's scope notes); the `Output:` line is
 * appended here and nowhere else. `outRel` comes from the CALLER — it computes
 * the name once (via `traceOutputRel`) so the saved request document, the
 * pending-run registry and the run's output all agree on one identity.
 * Never throws — `agent-missing` is told apart by the `agent_unavailable:`
 * prefix the host puts on the message.
 */
export async function delegateTrace(
  text: string,
  vaultRoot: string,
  /** Vault-relative report path this run must write (`traceOutputRel(...)`). */
  outRel: string,
  /** Which agent should run it; omitted = whatever the host would pick. */
  harness?: string,
): Promise<DelegateResult> {
  await seed()
  const outAbs = absolute(vaultRoot, outRel)
  try {
    const { run_id } = await agentRun({
      task: TRACE_TASK_ID,
      ...(harness ? { harness } : {}),
      prompt: `${text.replace(/\s+$/, '')}\n\nOutput: ${outRel}\n`,
      notify: {
        title_ok: t('notifyOk'),
        title_fail: t('notifyFail'),
        open_path: outAbs,
        expect_file: outAbs,
      },
    })
    // A missing run id must read as failure: reporting `ok` would tell the
    // user a run is on its way when nothing can ever be reconciled or notified.
    if (typeof run_id !== 'string' || run_id === '') {
      return { ok: false, reason: 'error', message: 'the agent started a run without a run id' }
    }
    return { ok: true, runId: run_id, outRel }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return {
      ok: false,
      reason: message.includes('agent_unavailable') ? 'agent-missing' : 'error',
      message,
    }
  }
}

/** How often the window asks a run how it is doing (same cadence as
 *  idea-spark; the interval belongs to the WINDOW — closing it stops the
 *  polling and nothing else, the reminder still arrives). */
export const POLL_MS = 2000

export type RunView =
  | { kind: 'running'; steps: number; last: string }
  | { kind: 'done'; success: boolean; message: string }
  | { kind: 'lost' }

/**
 * Reads one `host.agent.status` answer. Deliberately total: ANY shape that
 * isn't recognisably `running` or a `done` carrying a record reads as `lost`
 * — we have no evidence the run is alive, and the UI's response to `lost` is
 * to stop waiting, not to destroy anything. (Verbatim from idea-spark's
 * agent-client, which pinned this behavior first.)
 */
export function interpretStatus(raw: unknown): RunView {
  if (raw === null || typeof raw !== 'object') return { kind: 'lost' }
  const o = raw as Record<string, unknown>

  if (o.state === 'running') {
    return {
      kind: 'running',
      steps: typeof o.steps === 'number' && Number.isFinite(o.steps) ? o.steps : 0,
      last: typeof o.last === 'string' ? o.last : '',
    }
  }

  if (o.state === 'done') {
    const rec = o.record
    if (rec === null || typeof rec !== 'object') return { kind: 'lost' }
    const r = rec as Record<string, unknown>
    const success = r.status === 'success'
    const text = success ? r.result : (r.stderr_tail ?? r.result)
    return { kind: 'done', success, message: typeof text === 'string' ? text : '' }
  }

  return { kind: 'lost' }
}
