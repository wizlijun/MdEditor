// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-fs', () => ({
  stat: vi.fn(), readTextFile: vi.fn(), mkdir: vi.fn(), writeTextFile: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: vi.fn() }))
vi.mock('../hash', () => ({ sha256Hex: vi.fn(async () => 'deadbeef') }))
vi.mock('../settings.svelte', () => ({
  settings: {}, loadSettings: vi.fn(async () => {}), getPluginScopedAll: vi.fn(() => ({})),
}))
vi.mock('../theme-loader', () => ({ computeActiveThemeId: vi.fn() }))
vi.mock('../plugins/share-baker', () => ({ bakeShareHtml: vi.fn() }))
vi.mock('../plugins/host-render-html', () => ({ renderTabAsInlineBody: vi.fn() }))
vi.mock('../share/publish', () => ({ publishHtml: vi.fn() }))
vi.mock('../share/unpublish', () => ({ unpublish: vi.fn() }))
vi.mock('../share/copy-link', () => ({ copyShareLink: vi.fn() }))
vi.mock('../share/upload-image', () => ({ uploadImage: vi.fn() }))
vi.mock('../share/records', () => ({ getRecord: vi.fn() }))
vi.mock('../insights/run', () => ({ generateInsightsReport: vi.fn() }))

import CliRunner from './CliRunner.svelte'
import { invoke } from '@tauri-apps/api/core'
import { stat, readTextFile } from '@tauri-apps/plugin-fs'
import { renderTabAsInlineBody } from '../plugins/host-render-html'
import type { CliPayload } from './cli-runner'
import type { CliEntry, PluginManifest } from '../plugins/types'

describe('CliRunner plugin path arguments', () => {
  let component: ReturnType<typeof mount> | null = null
  let target: HTMLDivElement

  beforeEach(() => {
    vi.clearAllMocks()
    target = document.createElement('div')
    document.body.append(target)
    vi.mocked(stat).mockResolvedValue({ mtime: new Date('2026-09-06T00:00:00Z') } as Awaited<ReturnType<typeof stat>>)
    vi.mocked(readTextFile).mockResolvedValue('# Meeting')
    vi.mocked(renderTabAsInlineBody).mockResolvedValue('<h1>Meeting</h1>')
  })

  afterEach(async () => {
    if (component) await unmount(component)
    component = null
    target.remove()
  })

  async function run(entry: CliEntry, source: string, capabilities: PluginManifest['host_capabilities'] = []) {
    const payload: CliPayload = {
      plugin_id: 'notemd.fixture', subcommand: entry.subcommand, plugin_command: entry.command,
      args: { source }, flags: { 'dry-run': true },
      global: { json: true, quiet: false, clipboard: false },
    }
    const manifest: PluginManifest = {
      id: payload.plugin_id, name: 'Fixture', version: '1.0.0', binary: '',
      cli: [entry], host_capabilities: capabilities,
    }
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case 'cli_payload': return payload
        case 'get_plugin_manifests': return [manifest]
        case 'plugin_v2_execute_cli': return { synced: 1 }
        case 'cli_finish': return undefined
        default: throw new Error(`unexpected invoke: ${command}`)
      }
    })
    component = mount(CliRunner, { target })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('cli_finish', {
      result: { exit_code: 0, stdout: '{"ok":true,"data":{"synced":1}}', stderr: [] },
    }))
  }

  const entry: CliEntry = {
    subcommand: 'fixture-sync', command: 'sync', summary: 'Sync a directory',
    args: [{ name: 'source', type: 'path', required: false }],
  }

  it('forwards a directory source without reading it when tab context is explicitly disabled', async () => {
    vi.mocked(readTextFile).mockRejectedValue(new Error('EISDIR: illegal operation on a directory'))
    await run({ ...entry, requires_tab_context: false }, '/tmp/hemory-vault')

    expect(stat).not.toHaveBeenCalled()
    expect(readTextFile).not.toHaveBeenCalled()
    expect(renderTabAsInlineBody).not.toHaveBeenCalled()
    expect(invoke).toHaveBeenCalledWith('plugin_v2_execute_cli', {
      pluginId: 'notemd.fixture', subcommand: 'fixture-sync', command: 'sync',
      context: expect.objectContaining({
        cli: { args: { source: '/tmp/hemory-vault' }, flags: { 'dry-run': true } },
        tab: expect.objectContaining({ path: '', is_untitled: true }),
      }),
    })
  })

  it('still reads and renders a file when tab context is required', async () => {
    await run({ ...entry, requires_tab_context: true }, '/tmp/meeting.md', ['renderer.raw', 'renderer.html'])

    expect(stat).toHaveBeenCalledWith('/tmp/meeting.md')
    expect(readTextFile).toHaveBeenCalledWith('/tmp/meeting.md')
    expect(renderTabAsInlineBody).toHaveBeenCalledOnce()
    expect(invoke).toHaveBeenCalledWith('plugin_v2_execute_cli', expect.objectContaining({
      context: expect.objectContaining({
        raw_content: '# Meeting', rendered_html: '<h1>Meeting</h1>',
        tab: expect.objectContaining({ path: '/tmp/meeting.md', is_untitled: false }),
      }),
    }))
  })

  it('keeps file loading when the runner receives an entry with no tab context field', async () => {
    await run(entry, '/tmp/legacy.md', ['renderer.raw'])

    expect(stat).toHaveBeenCalledWith('/tmp/legacy.md')
    expect(readTextFile).toHaveBeenCalledWith('/tmp/legacy.md')
    expect(invoke).toHaveBeenCalledWith('plugin_v2_execute_cli', expect.objectContaining({
      context: expect.objectContaining({
        raw_content: '# Meeting',
        tab: expect.objectContaining({ path: '/tmp/legacy.md', is_untitled: false }),
      }),
    }))
  })
})
