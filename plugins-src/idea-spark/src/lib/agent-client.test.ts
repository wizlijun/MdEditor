// The delegation call itself: what goes out on `host.agent.run`, and how the
// three shapes of `host.agent.status` are read back.
//
// Every assertion here is about a contract that fails SILENTLY or LOUDLY at
// the far end of the bridge and can't be caught by types:
//   * a `notify` spec missing any of its four fields makes claude-agent reject
//     the whole run (`NotifySpec` has no serde defaults);
//   * `note_path` / `open_path` / `expect_file` must be absolute, and
//     `note_path` must already exist (claude-agent canonicalizes it);
//   * `task` must be sent to `host.agent.status`, whose own default is another
//     plugin's task (`answer-note-question`).
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { delegateIdea, interpretStatus, POLL_MS, TASK_ID } from './agent-client'
import { TASK_FILES } from './task-template'

// This package has no jsdom (nothing else in it needs a DOM), so vitest runs
// these in plain node and the `window` the bridge reads has to be stood up by
// hand. Aliasing it to `globalThis` is the whole of it: `window.notemd` and
// `globalThis.notemd` then name the same property.
type Global = typeof globalThis & { notemd?: unknown }
const g = globalThis as Global
// `window` is DOM-typed (`Window & typeof globalThis`) but simply absent at
// run time here, so the alias is written through an `unknown`-typed view.
const withWindow = g as unknown as { window?: unknown }
withWindow.window ??= g

/** Installs a bridge whose `request` is the given mock. */
function useBridge(request: ReturnType<typeof vi.fn>): void {
  g.notemd = {
    pluginId: 'notemd.idea-spark',
    locale: 'en',
    theme: 'default',
    request,
    onMessage: () => {},
  }
}

beforeEach(() => {
  delete g.notemd
})

describe('delegateIdea', () => {
  it('sends the run with a complete notify spec and absolute paths', async () => {
    const request = vi.fn().mockResolvedValue({ run_id: 'r1' })
    useBridge(request)

    const r = await delegateIdea('inbox/ideas/a.md', '我的想法', '/V')

    expect(r).toEqual({ ok: true, runId: 'r1' })
    const [method, params] = request.mock.calls.at(-1)!
    expect(method).toBe('host.agent.run')
    expect(params.task).toBe('idea-proof')
    expect(params.note_path).toBe('/V/inbox/ideas/a.md')
    expect(params.notify.open_path).toBe('/V/inbox/ideas/a.proof.md')
    expect(params.notify.expect_file).toBe('/V/inbox/ideas/a.proof.md')
    expect(params.notify.title_ok).toContain('我的想法')
    expect(params.notify.title_fail).toContain('我的想法')
    // All four notify keys present: claude-agent rejects the run outright when
    // any one of them is missing, and the failure surfaces as "bad 'notify'".
    expect(Object.keys(params.notify).sort()).toEqual([
      'expect_file',
      'open_path',
      'title_fail',
      'title_ok',
    ])
  })

  it('trims a trailing slash off the vault root instead of doubling it', async () => {
    const request = vi.fn().mockResolvedValue({ run_id: 'r1' })
    useBridge(request)
    await delegateIdea('inbox/ideas/a.md', 't', '/V/')
    const [, params] = request.mock.calls.at(-1)!
    expect(params.note_path).toBe('/V/inbox/ideas/a.md')
  })

  it('seeds the task template before starting the run', async () => {
    // `exists` answers `{exists:false}` so every template file is written; the
    // run answers with a run id.
    const request = vi.fn(async (method: string) => {
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      return { run_id: 'r1' }
    })
    useBridge(request)

    await delegateIdea('inbox/ideas/a.md', 't', '/V')

    const methods = request.mock.calls.map(([m]) => m)
    const writes = request.mock.calls.filter(([m]) => m === 'host.vault.write')
    expect(writes).toHaveLength(Object.keys(TASK_FILES).length)
    // Order matters: claude-agent refuses an unknown task, so every template
    // file has to be on disk before the run is started.
    expect(methods.lastIndexOf('host.vault.write')).toBeLessThan(methods.indexOf('host.agent.run'))
  })

  it('still starts the run when seeding the template fails', async () => {
    // A read-only `.notemd/` must not cost the user a delegation they could
    // otherwise have made against an already-seeded template.
    const request = vi.fn(async (method: string) => {
      if (method === 'host.vault.exists' || method === 'host.vault.write') throw new Error('io: read-only')
      return { run_id: 'r1' }
    })
    useBridge(request)

    await expect(delegateIdea('inbox/ideas/a.md', 't', '/V')).resolves.toEqual({ ok: true, runId: 'r1' })
  })

  it('maps the agent_unavailable prefix to agent-missing', async () => {
    useBridge(vi.fn().mockRejectedValue(new Error('-32000: agent_unavailable: unknown v2 plugin')))
    const r = await delegateIdea('inbox/ideas/a.md', 't', '/V')
    expect(r).toMatchObject({ ok: false, reason: 'agent-missing' })
  })

  it('maps any other rejection to error, keeping the message', async () => {
    const request = vi.fn(async (method: string) => {
      if (method === 'host.agent.run') throw new Error("unknown task 'idea-proof'")
      return { exists: true }
    })
    useBridge(request)
    const r = await delegateIdea('inbox/ideas/a.md', 't', '/V')
    expect(r).toMatchObject({ ok: false, reason: 'error' })
    expect((r as { message: string }).message).toContain('idea-proof')
  })

  it('reports a run that answers without a run id as an error, never as started', async () => {
    // Reporting `ok` here would register a pending run keyed on `undefined`:
    // permanently ⏳, never reconciled, and it would survive a restart.
    useBridge(vi.fn().mockResolvedValue({}))
    const r = await delegateIdea('inbox/ideas/a.md', 't', '/V')
    expect(r).toMatchObject({ ok: false, reason: 'error' })
  })
})

describe('interpretStatus', () => {
  it('interprets every status shape', () => {
    expect(interpretStatus({ state: 'running', steps: 3, last: 'Read a.md' })).toEqual({
      kind: 'running',
      steps: 3,
      last: 'Read a.md',
    })
    expect(interpretStatus({ state: 'done', record: { status: 'success', result: 'ok' } })).toEqual({
      kind: 'done',
      success: true,
      message: 'ok',
    })
    expect(interpretStatus({ state: 'done', record: { status: 'error', stderr_tail: 'boom' } })).toEqual({
      kind: 'done',
      success: false,
      message: 'boom',
    })
    expect(interpretStatus({ state: 'lost' })).toEqual({ kind: 'lost' })
    expect(interpretStatus({ nonsense: 1 })).toEqual({ kind: 'lost' })
  })

  it('tolerates a running snapshot with no progress written yet', () => {
    // `steps`/`last` come from a progress file the run may not have written
    // in its first seconds; claude-agent sends 0 and "" then, but a missing
    // key must not render as "NaN steps".
    expect(interpretStatus({ state: 'running' })).toEqual({ kind: 'running', steps: 0, last: '' })
  })

  it('treats every non-success terminal status as a failure', () => {
    for (const status of ['error', 'timeout', 'cancelled', 'skipped']) {
      expect(interpretStatus({ state: 'done', record: { status } })).toMatchObject({
        kind: 'done',
        success: false,
      })
    }
  })

  it('is lost for null, a string, and a done with no record', () => {
    expect(interpretStatus(null)).toEqual({ kind: 'lost' })
    expect(interpretStatus('done')).toEqual({ kind: 'lost' })
    expect(interpretStatus({ state: 'done' })).toEqual({ kind: 'lost' })
  })
})

describe('constants', () => {
  it('names the task the template seeds, and polls on a human interval', () => {
    expect(TASK_ID).toBe('idea-proof')
    expect(POLL_MS).toBe(2000)
  })
})
