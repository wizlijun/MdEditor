import { describe, it, expect } from 'vitest'
import { basenameOf, extensionOf, inferKind } from './cli-runner'

describe('basenameOf / extensionOf / inferKind', () => {
  it('extracts basename and extension', () => {
    expect(basenameOf('/tmp/x.md')).toBe('x.md')
    expect(extensionOf('x.md')).toBe('.md')
  })
  it('infers markdown / html / code / image', () => {
    expect(inferKind('.md')).toBe('markdown')
    expect(inferKind('.HTML')).toBe('html')
    expect(inferKind('.ts')).toBe('code')
    expect(inferKind('.png')).toBe('image')
    expect(inferKind(null)).toBe('plaintext')
  })
})
