import { describe, expect, it, vi } from 'vitest'
import { runReadingInsightsCli, type ReadingInsightsCliDeps } from './reading-insights-cli'
import type { CliFinishResult } from './share-cli'
import type { CliPayload } from './cli-runner'

function payload(
  flags: CliPayload['flags'] = {},
  global: Partial<CliPayload['global']> = {},
): CliPayload {
  return {
    subcommand: 'report', plugin_id: 'reading-insights', plugin_command: 'report',
    args: {}, flags,
    global: { json: false, quiet: false, clipboard: true, ...global },
  }
}

function harness() {
  const results: CliFinishResult[] = []
  const deps: ReadingInsightsCliDeps = {
    finish: vi.fn(async (result) => { results.push(result) }),
    resolveVault: vi.fn(async () => '/vault'),
    generate: vi.fn(async () => ({ filename: 'stat/report.md', markdown: '# report' })),
    mkdir: vi.fn(async () => {}),
    writeTextFile: vi.fn(async () => {}),
    now: () => Date.UTC(2026, 8, 2, 12),
    timezoneOffsetMinutes: () => 0,
  }
  return { deps, results }
}

describe('runReadingInsightsCli', () => {
  it('rejects a lone --from instead of silently falling back to a preset', async () => {
    const { deps, results } = harness()
    await runReadingInsightsCli(payload({ from: '2026-09-01' }), deps)
    expect(results[0].exit_code).toBe(2)
    expect(results[0].stderr[0]).toContain('must be provided together')
    expect(deps.generate).not.toHaveBeenCalled()
  })

  it('validates exact dates and ordering', async () => {
    const { deps, results } = harness()
    await runReadingInsightsCli(payload({ from: '2026-09-03', to: '2026-09-01' }), deps)
    expect(results[0].exit_code).toBe(2)
    expect(results[0].stderr[0]).toContain('--from <= --to')
  })

  it('rejects --date together with an explicit range', async () => {
    const { deps, results } = harness()
    await runReadingInsightsCli(
      payload({ date: 'today', from: '2026-09-01', to: '2026-09-02' }),
      deps,
    )
    expect(results[0].exit_code).toBe(2)
    expect(results[0].stderr[0]).toContain('conflicts')
    expect(deps.generate).not.toHaveBeenCalled()
  })

  it('rejects empty date and vault values', async () => {
    const first = harness()
    await runReadingInsightsCli(payload({ from: '', to: '' }), first.deps)
    expect(first.results[0].exit_code).toBe(2)
    expect(first.deps.generate).not.toHaveBeenCalled()

    const second = harness()
    await runReadingInsightsCli(payload({ vault: '' }), second.deps)
    expect(second.results[0].exit_code).toBe(2)
    expect(second.deps.resolveVault).not.toHaveBeenCalled()
  })

  it('returns markdown in a JSON envelope for --stdout --json', async () => {
    const { deps, results } = harness()
    await runReadingInsightsCli(
      payload({ from: '2026-09-01', to: '2026-09-02', stdout: true }, { json: true }),
      deps,
    )
    expect(JSON.parse(results[0].stdout!)).toEqual({
      ok: true,
      data: { from: '2026-09-01', to: '2026-09-02', markdown: '# report' },
    })
  })

  it('writes through a Windows vault path and quiet suppresses human stdout', async () => {
    const { deps, results } = harness()
    ;(deps.resolveVault as ReturnType<typeof vi.fn>).mockResolvedValue('C:\\Vault')
    await runReadingInsightsCli(payload({}, { quiet: true }), deps)
    expect(deps.mkdir).toHaveBeenCalledWith('C:\\Vault\\stat')
    expect(deps.writeTextFile).toHaveBeenCalledWith('C:\\Vault\\stat\\report.md', '# report')
    expect(results[0]).toEqual({ exit_code: 0, stdout: undefined, stderr: [] })
  })

  it('returns structured failures under --json', async () => {
    const { deps, results } = harness()
    ;(deps.resolveVault as ReturnType<typeof vi.fn>).mockResolvedValue(null)
    await runReadingInsightsCli(payload({}, { json: true }), deps)
    expect(JSON.parse(results[0].stdout!)).toMatchObject({
      ok: false,
      error: { code: 'vault_required' },
    })
    expect(results[0].stderr).toEqual([])
  })
})
