import { describe, expect, it } from 'vitest'
import manifest from '../manifest.v2.json'

describe('meetings manifest', () => {
  it('routes the UI command and Hemory CLI command through the native plugin', () => {
    expect(manifest.id).toBe('notemd.meetings')
    expect(manifest.binary['aarch64-apple-darwin']).toBe('bin/notemd-meetings')
    expect(manifest.contributes.windows[0].open_command).toBe('open')
    expect(manifest.activation.events).toContain('onCommand:open')

    const cli = manifest.contributes.cli[0]
    expect(cli.subcommand).toBe('meetings-import-hemory')
    expect(cli.command).toBe('import-hemory')
    expect(cli.args).toContainEqual(expect.objectContaining({ name: 'source', type: 'path', required: false }))
    expect(manifest.activation.events).toContain(`onCli:${cli.subcommand}`)
    expect(manifest.capabilities).toEqual(expect.arrayContaining([
      'vault.read',
      'vault.write',
      'dialog',
      'toast',
    ]))
  })

  it('keeps Vault-scoped settings inside the plugin UI', () => {
    expect(manifest.contributes).not.toHaveProperty('settings')
    expect(manifest.capabilities).not.toContain('settings')
    expect(manifest.description).toContain('configurable Vault directory')
  })

  it('exposes one-command incremental sync without requiring a document', () => {
    const cli = manifest.contributes.cli.find((entry) => entry.subcommand === 'meetings-sync')!
    expect(cli.command).toBe('sync')
    expect(cli.requires_tab_context).toBe(false)
    expect(cli.args).toEqual([
      expect.objectContaining({ name: 'source', type: 'path', required: false }),
    ])
    expect(cli.flags.map((flag) => flag.long)).toEqual(['--dry-run', '--user', '--timezone'])
    expect(manifest.activation.events).toContain('onCli:meetings-sync')
  })
})
