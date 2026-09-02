import { describe, expect, it } from 'vitest'
import manifest from '../manifest.v2.json'
import packageJson from '../package.json'

describe('Memory plugin manifest', () => {
  it('ships the Protocol v2 maintenance release on its compatible Host', () => {
    expect(manifest.id).toBe('notemd.memory')
    expect(manifest.version).toBe('2.1.0')
    expect(packageJson.version).toBe(manifest.version)
    expect(manifest.engines.notemd).toBe('>=6.902.1')
    expect(manifest.capabilities).toContain('memory.control')
    expect(manifest.capabilities).toEqual(expect.arrayContaining(['agent', 'vault.read', 'vault.write']))
  })
})
