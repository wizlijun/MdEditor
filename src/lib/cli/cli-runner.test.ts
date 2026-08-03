import { describe, it, expect } from 'vitest'
import { basenameOf, extensionOf, inferKind, requiresFileArg } from './cli-runner'
import type { CliEntry } from '../plugins/types'

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

describe('requiresFileArg', () => {
  it('is true when the entry declares a required path arg', () => {
    expect(requiresFileArg({ args: [{ type: 'path', required: true }] })).toBe(true)
  })
  it('is false for a file-less subcommand', () => {
    expect(requiresFileArg({ args: [] })).toBe(false)
    expect(requiresFileArg(undefined)).toBe(false)
  })
  it('is false when the path arg is optional', () => {
    expect(requiresFileArg({ args: [{ type: 'path', required: false }] })).toBe(false)
  })
  it('ignores required args that are not paths', () => {
    expect(requiresFileArg({ args: [{ type: 'string', required: true }] })).toBe(false)
  })
  it('accepts a real CliEntry (manifest shape has extra fields beyond type/required)', () => {
    const entry: CliEntry = {
      subcommand: 'export', command: 'export', summary: 'Export as PDF',
      args: [{ name: 'file', type: 'path', required: true, help: 'the file to export' }],
    }
    expect(requiresFileArg(entry)).toBe(true)
  })
})
