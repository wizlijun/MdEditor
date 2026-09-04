import { describe, expect, it } from 'vitest'
import manifest from '../manifest.v2.json'
import packageJson from '../package.json'

describe('Memory plugin manifest', () => {
  it('ships Role/Scope governance on its compatible Host', () => {
    expect(manifest.id).toBe('notemd.memory')
    expect(manifest.version).toBe('2.4.4')
    expect(packageJson.version).toBe(manifest.version)
    expect(manifest.engines.notemd).toBe('>=6.904.3')
    expect(manifest.capabilities).toContain('memory.control')
    expect(manifest.capabilities).toEqual(expect.arrayContaining(['agent', 'clipboard.write', 'vault.read', 'vault.write', 'editor.kit']))
  })
})
