import { describe, expect, it } from 'vitest'
import manifest from '../manifest.v2.json'
import packageJson from '../package.json'

describe('Memory plugin manifest', () => {
  it('ships the Protocol v2 maintenance release on its compatible Host', () => {
    expect(manifest.id).toBe('notemd.memory')
    expect(manifest.version).toBe('2.0.1')
    expect(packageJson.version).toBe(manifest.version)
    expect(manifest.engines.notemd).toBe('>=6.901.10')
    expect(manifest.capabilities).toContain('memory.control')
  })
})
