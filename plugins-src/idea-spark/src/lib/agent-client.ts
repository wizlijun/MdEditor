// agent-client.ts — the delegation call and the reading of a run's status.
//
// The whole chain in one paragraph: the editor's buffer is flushed to disk,
// the `idea-proof` task template is seeded into the vault (idempotent, see
// task-template.ts), and `host.agent.run` hands claude-agent the idea's
// ABSOLUTE path plus the tray reminder it should push when the run ends. The
// host relays that call verbatim to the resident `notemd.claude-agent` plugin.
// This window then polls `host.agent.status` purely for its own inline
// progress display — the *reminder* is claude-agent's job, precisely because
// this window may well be closed by the time the run finishes.
//
// No `$state` and no store access here: this module is IO + pure mapping, so
// its tests need nothing but a stubbed `window.notemd`.
import { agentRun, agentStatus, vaultExists, vaultWrite } from './bridge'
import { proofPathFor } from './naming'
import { seedTaskTemplate, TASK_ID, type SeedIo } from './task-template'
import { t } from './strings'

export { TASK_ID }

/**
 * How often the window asks a run how it is doing. Two seconds: fast enough
 * that "arguing…" feels live, slow enough that a run reading a large vault for
 * twenty minutes costs a few hundred cheap round trips, each of which is three
 * small file reads inside claude-agent.
 *
 * This interval belongs to the WINDOW, not to the run. Closing the window
 * stops the polling and nothing else — the run keeps going and still delivers
 * its tray reminder.
 */
export const POLL_MS = 2000

export type DelegateResult =
  | { ok: true; runId: string }
  | { ok: false; reason: 'agent-missing' | 'error'; message: string }

export type RunView =
  | { kind: 'running'; steps: number; last: string }
  | { kind: 'done'; success: boolean; message: string }
  | { kind: 'lost' }

/** `('/V', 'inbox/ideas/a.md')` → `/V/inbox/ideas/a.md`. Both a trailing slash
 *  on the root and a leading one on the relative path are tolerated: a
 *  `//` in the middle would survive into `canonicalize` on some platforms. */
function absolute(root: string, rel: string): string {
  return `${root.replace(/\/+$/, '')}/${rel.replace(/^\/+/, '')}`
}

/** Seeding IO backed by the bridge. A failed existence check counts as "not
 *  there" so the write is at least attempted; the write's own failure is what
 *  `seed` below swallows. */
const seedIo: SeedIo = {
  exists: (path) => vaultExists(path).then((r) => r.exists === true),
  write: async (path, content) => {
    await vaultWrite(path, content)
  },
}

/**
 * Puts the `idea-proof` task template on disk if it isn't there already, and
 * never throws.
 *
 * Best-effort on purpose: a vault whose `.notemd/` can't be written (read-only
 * mount, permissions) must not cost the user a delegation against a template
 * that was seeded on an earlier run. The authoritative complaint about a
 * missing template is claude-agent's own `unknown task 'idea-proof'`, which
 * comes back through `delegateIdea`'s error path with the message intact.
 */
async function seed(): Promise<void> {
  try {
    await seedTaskTemplate(seedIo)
  } catch (e) {
    console.warn('[idea-spark] seeding the idea-proof task template failed:', e)
  }
}

/**
 * Starts a run against one idea.
 *
 * `ideaRel` is vault-relative (the store's key convention) and `vaultRoot` is
 * absolute; everything crossing the bridge is built from the two, because
 * claude-agent `canonicalize`s `note_path` and the host resolves the
 * reminder's `open_path` against the vault. The file at `ideaRel` MUST already
 * exist — the caller flushes the editor before getting here.
 *
 * `title` is the idea's own title (the inbox's row label), and it goes into
 * both reminder titles: a tray notification saying "Idea argued" means nothing
 * an hour later if it doesn't say *which* idea.
 *
 * Never throws. `agent-missing` is the case worth telling apart — claude-agent
 * isn't installed or couldn't be activated — and the only signal for it is the
 * `agent_unavailable:` prefix the host puts on the message (the code is the
 * generic -32000; there is no dedicated one).
 */
export async function delegateIdea(
  ideaRel: string,
  title: string,
  vaultRoot: string,
): Promise<DelegateResult> {
  await seed()

  const proofAbs = absolute(vaultRoot, proofPathFor(ideaRel))
  try {
    const { run_id } = await agentRun({
      task: TASK_ID,
      prompt: `idea: ${ideaRel}\nproof: ${proofPathFor(ideaRel)}`,
      note_path: absolute(vaultRoot, ideaRel),
      notify: {
        title_ok: `${t('notifyOk')} · ${title}`,
        title_fail: `${t('notifyFail')} · ${title}`,
        open_path: proofAbs,
        expect_file: proofAbs,
      },
    })
    // A run id is the only thing that makes a pending run reconcilable later.
    // Without one, `markPending` would key on `undefined`: a ⏳ that no status
    // call can ever resolve and that survives every restart.
    if (typeof run_id !== 'string' || run_id === '') {
      return { ok: false, reason: 'error', message: 'the agent started a run without a run id' }
    }
    return { ok: true, runId: run_id }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return {
      ok: false,
      reason: message.includes('agent_unavailable') ? 'agent-missing' : 'error',
      message,
    }
  }
}

/**
 * Reads one `host.agent.status` answer.
 *
 * Deliberately total: ANY shape that isn't recognisably `running` or a `done`
 * carrying a record is reported as `lost`. `lost` is claude-agent's own word
 * for "no record and no live lock" — the runner died without writing anything
 * — and treating an unparseable answer the same way is the honest reading: we
 * have no evidence the run is still alive, and the UI's response to `lost` is
 * to stop waiting, not to destroy anything.
 *
 * `message` is the run's `result` on success and its `stderr_tail` on failure,
 * which is what the record actually carries in each case; it is only ever used
 * as toast/console text, never parsed.
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
    // `done` with no record is not a terminal outcome we can report on — it is
    // an answer we don't understand, so it reads as `lost` like any other.
    if (rec === null || typeof rec !== 'object') return { kind: 'lost' }
    const r = rec as Record<string, unknown>
    const success = r.status === 'success'
    const text = success ? r.result : (r.stderr_tail ?? r.result)
    return { kind: 'done', success, message: typeof text === 'string' ? text : '' }
  }

  return { kind: 'lost' }
}
