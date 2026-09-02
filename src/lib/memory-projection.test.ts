import { describe, expect, it } from 'vitest'
import { isMemoryProjectionPath } from './memory-projection'

describe('isMemoryProjectionPath', () => {
  it('reserves only USER.md and MEMORY.md at the configured Vault root', () => {
    expect(isMemoryProjectionPath('/vault/USER.md', '/vault')).toBe(true)
    expect(isMemoryProjectionPath('/vault/MEMORY.md', '/vault/')).toBe(true)
    expect(isMemoryProjectionPath('C:\\vault\\MEMORY.md', 'C:\\vault')).toBe(true)
  })

  it('does not inspect legacy content markers or reserve nested/unconfigured files', () => {
    expect(isMemoryProjectionPath('/archive/USER.md', '/vault')).toBe(false)
    expect(isMemoryProjectionPath('/vault/notes/MEMORY.md', '/vault')).toBe(false)
    expect(isMemoryProjectionPath('/vault/MEMORY.md', null)).toBe(false)
  })
})
