import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import {
  activeProvider,
  agentPluginAvailable,
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
  const manifest = (id: string, commands: string[]) => ({
    id,
    activation: { events: commands.map((c) => `onCommand:${c}`) },
  })
  const AGENT = ['run-task', 'run-note', 'run-status']

  function install(...ms: Array<{ id: string; activation: { events: string[] } }>) {
    // pluginRuntime.manifests is the host's live registry; swap it for the test.
    ;(pluginRuntime as unknown as { manifests: unknown[] }).manifests = ms
  }

  afterEach(() => install())

  it('recognizes a plugin that declares all three agent commands', () => {
    install(manifest('notemd.claude-agent', AGENT))
    expect(agentProviders()).toEqual(['notemd.claude-agent'])
    expect(agentPluginAvailable()).toBe(true)
  })

  it('does not recognize a plugin missing one of them', () => {
    install(manifest('notemd.half', ['run-task', 'run-status']))
    expect(agentProviders()).toEqual([])
    expect(agentPluginAvailable()).toBe(false)
  })

  it('lists providers with the default first, then sorted', () => {
    install(
      manifest('notemd.zzz-agent', AGENT),
      manifest('notemd.deepseek-agent', AGENT),
      manifest('notemd.md2pdf', ['export']),
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
