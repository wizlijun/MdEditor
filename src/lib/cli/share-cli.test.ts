import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock every side-effectful module runShareCli touches; ShareError stays REAL
// ('../share/types' is not mocked) so instanceof-based error mapping is pinned.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-fs', () => ({ stat: vi.fn(), readTextFile: vi.fn() }))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))
vi.mock('../hash', () => ({ sha256Hex: vi.fn(async () => 'deadbeef') }))
vi.mock('../settings.svelte', () => ({
  settings: { theme: { light: 'paper-light', dark: 'ink-dark', followSystem: false } },
}))
vi.mock('../theme-loader', () => ({ computeActiveThemeId: vi.fn(() => 'paper-light') }))
vi.mock('../plugins/share-baker', () => ({ bakeShareHtml: vi.fn() }))
vi.mock('../share', () => ({ getShareConfig: vi.fn(), prepareShareSrc: vi.fn() }))
vi.mock('../sotvault.svelte', () => ({ refreshSotvault: vi.fn() }))
vi.mock('../share/publish', () => ({ publishHtml: vi.fn() }))
vi.mock('../share/unpublish', () => ({ unpublish: vi.fn() }))
vi.mock('../share/copy-link', () => ({ copyShareLink: vi.fn() }))
vi.mock('../share/upload-image', () => ({ uploadImage: vi.fn() }))
vi.mock('../share/records', () => ({ getRecord: vi.fn() }))

import { runShareCli, type CliFinishResult } from './share-cli'
import { ShareError } from '../share/types'
import type { CliPayload } from './cli-runner'
import { stat, readTextFile } from '@tauri-apps/plugin-fs'
import { getShareConfig, prepareShareSrc } from '../share'
import { bakeShareHtml } from '../plugins/share-baker'
import { publishHtml } from '../share/publish'
import { unpublish } from '../share/unpublish'
import { copyShareLink } from '../share/copy-link'
import { getRecord } from '../share/records'

const CFG = {
  baseUrl: 'https://w',
  defaultExpiry: 'never' as const,
  slugRandomSuffix: true,
}

const HTML_RECORD = {
  slug: 's1', edit_token: 'tok', url: 'https://w/s1',
  created_at: '2026-07-01T00:00:00Z', expires_at: null, filename: 'a.md',
}

function payload(over: Partial<CliPayload> = {}, global: Partial<CliPayload['global']> = {}): CliPayload {
  return {
    subcommand: 'share', plugin_id: 'share', plugin_command: 'publish',
    args: { file: '/tmp/a.md' }, flags: {},
    global: { json: false, quiet: false, clipboard: true, ...global },
    ...over,
  }
}

function makeDeps() {
  const results: CliFinishResult[] = []
  const deps = {
    finish: vi.fn(async (r: CliFinishResult) => { results.push(r) }),
    systemDark: () => false,
    writeClipboard: vi.fn(async () => {}),
    diagnostics: vi.fn(async () => ['  diag: line']),
  }
  return { deps, results }
}

function envelope(r: CliFinishResult): any {
  return JSON.parse(r.stdout ?? 'null')
}

beforeEach(() => {
  vi.clearAllMocks()
  ;(getShareConfig as any).mockReturnValue(CFG)
  ;(stat as any).mockResolvedValue({ mtime: new Date('2026-07-01T00:00:00Z') })
  ;(readTextFile as any).mockResolvedValue('# hi')
  ;(prepareShareSrc as any).mockResolvedValue('notes/a.md')
  ;(bakeShareHtml as any).mockResolvedValue('<html>x</html>')
  ;(publishHtml as any).mockResolvedValue({ url: 'https://w/s1', slug: 's1', isUpdate: false })
  ;(getRecord as any).mockReturnValue(HTML_RECORD)
  ;(copyShareLink as any).mockResolvedValue('https://w/s1')
  ;(unpublish as any).mockResolvedValue(undefined)
})

describe('runShareCli publish', () => {
  it('emits the ok envelope {url,slug,is_update,created_at} with exit 0', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(results).toHaveLength(1)
    expect(results[0].exit_code).toBe(0)
    expect(envelope(results[0])).toEqual({
      ok: true,
      data: { url: 'https://w/s1', slug: 's1', is_update: false, created_at: '2026-07-01T00:00:00Z' },
    })
  })

  it('bakes with the active theme id, never the default', async () => {
    const { deps } = makeDeps()
    await runShareCli(payload(), deps)
    expect((bakeShareHtml as any).mock.calls[0][1]).toBe('paper-light')
  })

  it('non-json prints the bare url and writes the clipboard', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(payload(), deps)
    expect(results[0]).toMatchObject({ exit_code: 0, stdout: 'https://w/s1' })
    expect(deps.writeClipboard).toHaveBeenCalledWith('https://w/s1')
  })

  it('--json suppresses the publish clipboard write', async () => {
    const { deps } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(deps.writeClipboard).not.toHaveBeenCalled()
  })

  it('not_configured → exit 4 with code not_configured', async () => {
    ;(getShareConfig as any).mockReturnValue(null)
    const { deps, results } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(results[0].exit_code).toBe(4)
    expect(envelope(results[0])).toEqual({
      ok: false,
      error: { code: 'not_configured', message: 'share not configured (baseUrl/apiKey)' },
    })
  })

  it('ShareError(too_large) from bake → exit 4 with code too_large', async () => {
    ;(bakeShareHtml as any).mockRejectedValue(new ShareError('too_large', '26 MB'))
    const { deps, results } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(results[0].exit_code).toBe(4)
    expect(envelope(results[0]).error.code).toBe('too_large')
  })

  it('missing file → exit 2 (file error)', async () => {
    ;(stat as any).mockRejectedValue(new Error('ENOENT'))
    const { deps, results } = makeDeps()
    await runShareCli(payload(), deps)
    expect(results[0].exit_code).toBe(2)
    expect(results[0].stderr[0]).toContain("cannot read '/tmp/a.md'")
  })

  it('missing file BEATS unconfigured: exit 2, not 4 (old contract ordering)', async () => {
    ;(stat as any).mockRejectedValue(new Error('ENOENT'))
    ;(getShareConfig as any).mockReturnValue(null)
    const { deps, results } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(results[0].exit_code).toBe(2)
    expect(envelope(results[0]).error.code).toBe('invalid_arguments')
    expect(results[0].stderr).toEqual([])
    expect(getShareConfig).not.toHaveBeenCalled()
  })

  it('missing file argument → exit 2', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(payload({ args: {} }), deps)
    expect(results[0].exit_code).toBe(2)
    expect(results[0].stderr[0]).toContain('missing file argument')
  })

  it('vault_required under --json emits only the error envelope', async () => {
    ;(prepareShareSrc as any).mockRejectedValue(new ShareError('vault_required'))
    const { deps, results } = makeDeps()
    await runShareCli(payload({}, { json: true }), deps)
    expect(results[0].exit_code).toBe(4)
    expect(envelope(results[0]).error.code).toBe('vault_required')
    expect(results[0].stderr).toEqual([])
  })
})

describe('runShareCli copy-link', () => {
  const cl = (global: Partial<CliPayload['global']> = {}) =>
    payload({ plugin_command: 'copy-link' }, global)

  it('emits {url,slug} envelope, exit 0, and needs no config', async () => {
    ;(getShareConfig as any).mockReturnValue(null)  // config absent is fine
    const { deps, results } = makeDeps()
    await runShareCli(cl({ json: true }), deps)
    expect(results[0].exit_code).toBe(0)
    expect(envelope(results[0])).toEqual({ ok: true, data: { url: 'https://w/s1', slug: 's1' } })
  })

  it('clipboard gating: --json → copyShareLink told NOT to write', async () => {
    const { deps } = makeDeps()
    await runShareCli(cl({ json: true, clipboard: true }), deps)
    expect(copyShareLink).toHaveBeenCalledWith('/tmp/a.md', { clipboard: false })
  })

  it('clipboard gating: --no-clipboard → no write', async () => {
    const { deps } = makeDeps()
    await runShareCli(cl({ clipboard: false }), deps)
    expect(copyShareLink).toHaveBeenCalledWith('/tmp/a.md', { clipboard: false })
  })

  it('clipboard gating: default (tty, no --json) → write', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(cl(), deps)
    expect(copyShareLink).toHaveBeenCalledWith('/tmp/a.md', { clipboard: true })
    expect(results[0].stdout).toBe('https://w/s1')
  })

  it('--quiet suppresses the human success output', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(cl({ quiet: true }), deps)
    expect(results[0].stdout).toBeUndefined()
  })

  it('corrupt record → exit 4 with code corrupt_record', async () => {
    ;(copyShareLink as any).mockRejectedValue(new ShareError('corrupt_record', 'no url'))
    const { deps, results } = makeDeps()
    await runShareCli(cl({ json: true }), deps)
    expect(results[0].exit_code).toBe(4)
    expect(envelope(results[0]).error.code).toBe('corrupt_record')
  })
})

describe('runShareCli unpublish', () => {
  const up = (global: Partial<CliPayload['global']> = {}) =>
    payload({ plugin_command: 'unpublish' }, global)

  it('emits {removed:true,slug} envelope with exit 0', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(up({ json: true }), deps)
    expect(unpublish).toHaveBeenCalledWith({ path: '/tmp/a.md', baseUrl: 'https://w' })
    expect(results[0].exit_code).toBe(0)
    expect(envelope(results[0])).toEqual({ ok: true, data: { removed: true, slug: 's1' } })
  })

  it('not_configured → exit 4; never stats the file (old binary parity)', async () => {
    ;(getShareConfig as any).mockReturnValue(null)
    const { deps, results } = makeDeps()
    await runShareCli(up({ json: true }), deps)
    expect(results[0].exit_code).toBe(4)
    expect(envelope(results[0]).error.code).toBe('not_configured')
    expect(stat).not.toHaveBeenCalled()
  })

  it('non-json prints a human confirmation', async () => {
    const { deps, results } = makeDeps()
    await runShareCli(up(), deps)
    expect(results[0].stdout).toBe('unshared /tmp/a.md')
  })
})
