import { describe, expect, it } from 'vitest'
import { isManagedMemoryProjection } from './memory-projection'

describe('isManagedMemoryProjection', () => {
  it('recognises controlled USER and MEMORY projections on Unix and Windows paths', () => {
    expect(isManagedMemoryProjection(
      '/vault/USER.md',
      '---\nmanaged:\n  by: notemd.memory\n---\n',
    )).toBe(true)
    expect(isManagedMemoryProjection(
      'C:\\vault\\MEMORY.md',
      '<!-- notemd-memory-control -->\n',
    )).toBe(true)
  })

  it('recognises an uninitialised controlled template by its read-only notice', () => {
    expect(isManagedMemoryProjection(
      '/vault/MEMORY.md',
      '> **GENERATED / READ-ONLY.** Use the Memory plugin.\n',
    )).toBe(true)
  })

  it('does not reserve an ordinary same-named document without a control signal', () => {
    expect(isManagedMemoryProjection('/archive/USER.md', '# A user guide\n')).toBe(false)
  })

  it('does not treat a control marker in another file as a projection', () => {
    expect(isManagedMemoryProjection(
      '/vault/notes.md',
      '<!-- notemd-memory-control -->\n',
    )).toBe(false)
  })
})
