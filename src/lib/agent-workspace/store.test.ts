import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import {
  activeProvider,
  agentPluginAvailable,
  restoreProvider,
  agentProviders,
  agentRun,
  dismissRun,
  emptyRun,
  isAgentBusy,
  setProvider,
  startNoteRun,
  __setExecuteForTests,
  DEFAULT_PLUGIN_ID,
  POLL_MS,
} from './store.svelte'
import { pluginRuntime } from '../plugins/runtime.svelte'

/** Drive the poll timer forward by one tick and let its promises settle. */
async function tick() {
  await vi.advanceTimersByTimeAsync(POLL_MS)
  await Promise.resolve()
}

describe('agent workspace run', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    Object.assign(agentRun, emptyRun())
  })
  afterEach(() => {
    __setExecuteForTests(null)
    vi.useRealTimers()
  })

  it('starts a run and polls until the record lands', async () => {
    const calls: string[] = []
    let state = 'running'
    __setExecuteForTests(async (command) => {
      calls.push(command)
      if (command === 'run-note') return { run_id: 'R1' }
      if (state === 'running') {
        state = 'done'
        return { state: 'running', steps: 2, last: 'Read a.note.md' }
      }
      return {
        state: 'done',
        record: { status: 'success', result: 'answered 1', artifacts: ['answers/a.md'] },
      }
    })
    const finished = vi.fn()

    await startNoteRun('/v/a.note.md', finished)
    expect(agentRun.phase).toBe('running')
    expect(agentRun.runId).toBe('R1')
    expect(isAgentBusy()).toBe(true)
    expect(finished).not.toHaveBeenCalled()

    await tick()
    expect(agentRun.steps).toBe(2)
    expect(agentRun.last).toBe('Read a.note.md')
    expect(isAgentBusy()).toBe(true)

    await tick()
    expect(agentRun.phase).toBe('done')
    expect(agentRun.message).toBe('answered 1')
    expect(agentRun.artifacts).toEqual(['answers/a.md'])
    expect(isAgentBusy()).toBe(false)
    expect(finished).toHaveBeenCalledTimes(1)
    expect(calls).toEqual(['run-note', 'run-status', 'run-status'])
  })

  it('a failed run ends in error with the reason kept', async () => {
    __setExecuteForTests(async (command) => {
      if (command === 'run-note') return { run_id: 'R1' }
      return { state: 'done', record: { status: 'timeout', result: 'took too long' } }
    })
    const finished = vi.fn()
    await startNoteRun('/v/a.note.md', finished)
    await tick()
    expect(agentRun.phase).toBe('error')
    expect(agentRun.outcome).toBe('timeout')
    expect(agentRun.message).toBe('took too long')
    expect(finished).toHaveBeenCalledTimes(1)
  })

  it('a run that leaves no record stops polling instead of spinning forever', async () => {
    __setExecuteForTests(async (command) => {
      if (command === 'run-note') return { run_id: 'R1' }
      return { state: 'lost' }
    })
    const finished = vi.fn()
    await startNoteRun('/v/a.note.md', finished)
    await tick()
    expect(agentRun.phase).toBe('error')
    expect(agentRun.outcome).toBe('lost')
    expect(isAgentBusy()).toBe(false)
    await tick()
    expect(finished).toHaveBeenCalledTimes(1)
  })

  it('surfaces a plugin that refuses to start', async () => {
    __setExecuteForTests(async () => {
      throw new Error('no vault configured')
    })
    await startNoteRun('/v/a.note.md', vi.fn())
    expect(agentRun.phase).toBe('error')
    expect(agentRun.message).toBe('no vault configured')
    expect(isAgentBusy()).toBe(false)
  })

  it('treats a missing run id as a failure rather than a silent no-op', async () => {
    __setExecuteForTests(async () => ({}))
    await startNoteRun('/v/a.note.md', vi.fn())
    expect(agentRun.phase).toBe('error')
    expect(agentRun.message).toMatch(/run id/)
  })

  it('refuses to start a second run while one is in flight', async () => {
    const execute = vi.fn(async (command: string) => {
      if (command === 'run-note') return { run_id: 'R1' }
      return { state: 'running', steps: 0, last: '' }
    })
    __setExecuteForTests(execute)
    await startNoteRun('/v/a.note.md', vi.fn())
    await startNoteRun('/v/b.note.md', vi.fn())
    expect(agentRun.notePath).toBe('/v/a.note.md')
    expect(execute.mock.calls.filter((c) => c[0] === 'run-note')).toHaveLength(1)
  })

  it('dismiss clears a finished run but never an in-flight one', async () => {
    __setExecuteForTests(async (command) => {
      if (command === 'run-note') return { run_id: 'R1' }
      return { state: 'running', steps: 1, last: 'x' }
    })
    await startNoteRun('/v/a.note.md', vi.fn())
    await tick()
    dismissRun()
    expect(agentRun.phase).toBe('running')

    agentRun.phase = 'done'
    dismissRun()
    expect(agentRun.phase).toBe('idle')
    expect(agentRun.runId).toBe(null)
  })
})

describe('agent providers', () => {
  // The REAL view-model shape the host sends (plugin_runtime::adapter). It
  // carries no `activation` — the v6.817.4 regression was a filter written
  // against an invented shape that had one, which matched nothing and made the
  // whole Agent area vanish. Anything added here must exist in `to_v1`'s output.
  const manifest = (id: string, isAgent: boolean) => ({
    id,
    name: id,
    version: '1.0.0',
    binary: '',
    host_capabilities: [],
    agent_provider: isAgent,
  })
  const AGENT = true
  const NOT_AGENT = false

  function install(...ms: Array<ReturnType<typeof manifest>>) {
    // pluginRuntime.manifests is the host's live registry; swap it for the test.
    ;(pluginRuntime as unknown as { manifests: unknown[] }).manifests = ms
  }

  afterEach(() => install())

  it('recognizes a plugin the host flagged as a provider', () => {
    install(manifest('notemd.claude-agent', AGENT))
    expect(agentProviders()).toEqual(['notemd.claude-agent'])
    expect(agentPluginAvailable()).toBe(true)
  })

  it('does not recognize a plugin the host did not flag', () => {
    install(manifest('notemd.half', NOT_AGENT))
    expect(agentProviders()).toEqual([])
    expect(agentPluginAvailable()).toBe(false)
  })

  it('lists providers with the default first, then sorted', () => {
    install(
      manifest('notemd.zzz-agent', AGENT),
      manifest('notemd.deepseek-agent', AGENT),
      manifest('notemd.md2pdf', NOT_AGENT),
      manifest('notemd.claude-agent', AGENT),
    )
    expect(agentProviders()).toEqual([
      'notemd.claude-agent',
      'notemd.deepseek-agent',
      'notemd.zzz-agent',
    ])
  })

  it('dispatches to the default until told otherwise', () => {
    install(manifest('notemd.claude-agent', AGENT), manifest('notemd.deepseek-agent', AGENT))
    expect(activeProvider()).toBe('notemd.claude-agent')
    setProvider('notemd.deepseek-agent')
    expect(activeProvider()).toBe('notemd.deepseek-agent')
    setProvider(DEFAULT_PLUGIN_ID)
  })

  it('falls back when the chosen provider is uninstalled', () => {
    install(manifest('notemd.claude-agent', AGENT), manifest('notemd.deepseek-agent', AGENT))
    setProvider('notemd.deepseek-agent')
    install(manifest('notemd.claude-agent', AGENT))
    expect(activeProvider()).toBe('notemd.claude-agent')
    setProvider(DEFAULT_PLUGIN_ID)
  })

  it('serves from another provider when the default is not installed', () => {
    install(manifest('notemd.deepseek-agent', AGENT))
    expect(activeProvider()).toBe('notemd.deepseek-agent')
  })
})

describe('choosing an agent', () => {
  const manifest = (id: string, isAgent: boolean) => ({
    id,
    name: id,
    version: '1.0.0',
    binary: '',
    host_capabilities: [],
    agent_provider: isAgent,
  })
  function install(...ms: Array<ReturnType<typeof manifest>>) {
    ;(pluginRuntime as unknown as { manifests: unknown[] }).manifests = ms
  }

  // This suite runs without a DOM, so there is no `localStorage` global. The
  // store must work anyway — that is the point of `safeStorage()` — so nothing
  // here touches storage directly.
  beforeEach(() => {
    install(manifest('notemd.claude-agent', true), manifest('notemd.deepseek-agent', true))
    setProvider('notemd.claude-agent')
  })
  afterEach(() => install())

  /// The bug this exists for: picking DeepSeek looked like it did nothing.
  it('a choice takes effect immediately', () => {
    setProvider('notemd.deepseek-agent')
    expect(activeProvider()).toBe('notemd.deepseek-agent')
    setProvider('notemd.claude-agent')
    expect(activeProvider()).toBe('notemd.claude-agent')
  })

  /// With no storage to read, restoring must leave a working provider rather
  /// than blanking the picker.
  it('restoring without storage keeps a usable provider', () => {
    setProvider('notemd.deepseek-agent')
    restoreProvider()
    expect(['notemd.claude-agent', 'notemd.deepseek-agent']).toContain(activeProvider())
  })

  it('dispatches the run to the agent that was chosen', async () => {
    const seen: string[] = []
    __setExecuteForTests(async (command) => {
      seen.push(command)
      return { run_id: 'R1' }
    })
    setProvider('notemd.deepseek-agent')
    expect(activeProvider()).toBe('notemd.deepseek-agent')
    await startNoteRun('/v/a.note.md', () => {})
    expect(agentRun.provider).toBe('notemd.deepseek-agent')
    expect(seen).toContain('run-note')
    __setExecuteForTests(null)
    dismissRun()
  })
})
