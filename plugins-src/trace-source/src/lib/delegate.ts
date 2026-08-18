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
import { agentRun, vaultExists, vaultWrite } from './bridge'
import { seedTraceTemplate, TRACE_TASK_ID, type SeedIo } from './trace-template'
import { t } from './strings'

export type DelegateResult =
  | { ok: true; runId: string; outRel: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }

/** `('/V', 'traces/a.md')` → `/V/traces/a.md`, tolerating stray slashes. */
function absolute(root: string, rel: string): string {
  return `${root.replace(/\/+$/, '')}/${rel.replace(/^\/+/, '')}`
}

/** 摘要统一落 traces/,时间戳定名——调用方因此能预知 expect_file。 */
export function traceOutputRel(now: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const d = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`
  const t = `${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  return `traces/${d}-${t}.md`
}

const seedIo: SeedIo = {
  exists: (path) => vaultExists(path).then((r) => r.exists === true),
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
 * Starts one trace run. `text` is the composed delegation (quoted passage,
 * optional `Source-Doc:` line, the user's scope notes); the `Output:` line is
 * appended here and nowhere else. Never throws — `agent-missing` is told apart
 * by the `agent_unavailable:` prefix the host puts on the message.
 */
export async function delegateTrace(
  text: string,
  vaultRoot: string,
  /** Which agent should run it; omitted = whatever the host would pick. */
  harness?: string,
): Promise<DelegateResult> {
  await seed()
  const outRel = traceOutputRel(new Date())
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
