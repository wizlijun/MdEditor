import { describe, expect, it } from 'vitest'
import manifest from '../manifest.v2.json'
import packageJson from '../package.json'

describe('Memory plugin manifest', () => {
  it('ships Protocol v2 as plugin 2.0.0 on the first compatible Host', () => {
    expect(manifest.id).toBe('notemd.memory')
    expect(manifest.version).toBe('2.0.0')
    expect(packageJson.version).toBe(manifest.version)
    expect(manifest.engines.notemd).toBe('>=6.901.9')
    expect(manifest.capabilities).toContain('memory.control')
  })
})
