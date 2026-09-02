import { describe, it, expect } from 'vitest'
import { validateManifest, buildRegistry, findCliConflicts } from './registry'
import type { PluginManifest } from './types'

const valid = (over: Partial<PluginManifest> = {}): PluginManifest => ({
  id: 'share',
  name: 'Share',
  version: '1.0.0',
  binary: 'bin',
  host_capabilities: ['toast'],
  ...over,
})

describe('validateManifest', () => {
  it('accepts a minimal valid manifest', () => {
    expect(validateManifest(valid())).toEqual({ ok: true, value: valid() })
  })
  it('rejects missing id', () => {
    const bad = { ...valid() } as Partial<PluginManifest>; delete bad.id
    expect(validateManifest(bad).ok).toBe(false)
  })
  it('rejects non-kebab-case id', () => {
    expect(validateManifest(valid({ id: 'My_Plugin' })).ok).toBe(false)
    expect(validateManifest(valid({ id: 'a' })).ok).toBe(true)
    expect(validateManifest(valid({ id: 'a-b-1' })).ok).toBe(true)
  })
  it('rejects unknown capability', () => {
    expect(validateManifest(valid({ host_capabilities: ['mystery' as never] })).ok).toBe(false)
  })
  it('accepts settings.write:<scope> capability', () => {
    expect(validateManifest(valid({ host_capabilities: ['settings.write:share.records'] })).ok).toBe(true)
    expect(validateManifest(valid({ host_capabilities: ['settings.write:share.*'] })).ok).toBe(true)
  })
  it('rejects when settings keys do not match plugin id prefix', () => {
    const m = valid({
      settings: { tab_label: 'Share', schema: [{ key: 'other.foo', type: 'string', label: 'X' }] },
    })
    expect(validateManifest(m).ok).toBe(false)
  })
})

describe('buildRegistry', () => {
  it('rejects duplicate ids', () => {
    const result = buildRegistry([valid(), valid()])
    expect(result.errors.length).toBeGreaterThan(0)
    expect(Object.keys(result.byId).length).toBe(1)
  })
  it('keeps first wins on duplicate', () => {
    const a = valid({ name: 'first' })
    const b = valid({ name: 'second' })
    const r = buildRegistry([a, b])
    expect(r.byId['share'].name).toBe('first')
  })
})

describe('manifest cli validation', () => {
  const base = {
    id: 'demo',
    name: 'Demo',
    version: '0.1.0',
    binary: 'bin',
    host_capabilities: [] as string[],
  }

  it('accepts a well-formed cli entry', () => {
    const r = validateManifest({
      ...base,
      cli: [{ subcommand: 'demo', command: 'noop', summary: 's' }],
    })
    expect(r.ok).toBe(true)
  })

  it('rejects subcommand that collides with a builtin', () => {
    const r = validateManifest({
      ...base,
      cli: [{ subcommand: 'help', command: 'noop', summary: 's' }],
    })
    expect(r.ok).toBe(false)

    const currentCore = validateManifest({
      ...base,
      cli: [{ subcommand: 'search', command: 'noop', summary: 's' }],
    })
    expect(currentCore.ok).toBe(false)
  })

  it('rejects subcommand with bad characters', () => {
    const r = validateManifest({
      ...base,
      cli: [{ subcommand: 'Bad Name', command: 'noop', summary: 's' }],
    })
    expect(r.ok).toBe(false)
  })

  it('rejects alias that does not start with "-"', () => {
    const r = validateManifest({
      ...base,
      cli: [{ subcommand: 'demo', aliases: ['ess'], command: 'noop', summary: 's' }],
    })
    expect(r.ok).toBe(false)
  })

  it('rejects alias that collides with a reserved global flag', () => {
    const r = validateManifest({
      ...base,
      cli: [{ subcommand: 'demo', aliases: ['--json'], command: 'noop', summary: 's' }],
    })
    expect(r.ok).toBe(false)

    const modeFlag = validateManifest({
      ...base,
      cli: [{ subcommand: 'demo', aliases: ['--cli'], command: 'noop', summary: 's' }],
    })
    expect(modeFlag.ok).toBe(false)
  })

  it('rejects flag short that collides with a reserved global flag', () => {
    const r = validateManifest({
      ...base,
      cli: [{
        subcommand: 'demo', command: 'noop', summary: 's',
        flags: [{ long: '--update', short: '-h', type: 'boolean' }],
      }],
    })
    expect(r.ok).toBe(false)
  })
})

describe('findCliConflicts', () => {
  const builtins = ['help', 'version', 'plugin']

  function m(id: string, cli: PluginManifest['cli']): PluginManifest {
    return {
      id, name: id, version: '0.1.0', binary: 'bin',
      host_capabilities: [], cli,
    } as PluginManifest
  }

  it('returns empty when no conflicts', () => {
    const conflicts = findCliConflicts([
      m('a', [{ subcommand: 'one', command: 'x', summary: 's' }]),
      m('b', [{ subcommand: 'two', command: 'x', summary: 's' }]),
    ], builtins)
    expect(conflicts).toEqual([])
  })

  it('detects duplicate subcommand across plugins', () => {
    const conflicts = findCliConflicts([
      m('a', [{ subcommand: 'dup', command: 'x', summary: 's' }]),
      m('b', [{ subcommand: 'dup', command: 'x', summary: 's' }]),
    ], builtins)
    expect(conflicts).toHaveLength(1)
    expect(conflicts[0].kind).toBe('subcommand')
    expect(conflicts[0].owners.map(o => o.pluginId).sort()).toEqual(['a', 'b'])
  })

  it('detects duplicate alias across plugins', () => {
    const conflicts = findCliConflicts([
      m('a', [{ subcommand: 'one', aliases: ['-x'], command: 'x', summary: 's' }]),
      m('b', [{ subcommand: 'two', aliases: ['-x'], command: 'x', summary: 's' }]),
    ], builtins)
    expect(conflicts).toHaveLength(1)
    expect(conflicts[0].kind).toBe('alias')
    expect(conflicts[0].key).toBe('-x')
  })

  it('detects subcommand colliding with builtin even at registry level', () => {
    const conflicts = findCliConflicts([
      m('a', [{ subcommand: 'help', command: 'x', summary: 's' }]),
    ], builtins)
    expect(conflicts).toHaveLength(1)
    expect(conflicts[0].kind).toBe('subcommand')
    expect(conflicts[0].reservedCore).toBe(true)
  })
})

describe('buildRegistry drops cli entries from conflicting plugins', () => {
  function mfest(id: string, cli: PluginManifest['cli']): PluginManifest {
    return {
      id, name: id, version: '0.1.0', binary: 'bin',
      host_capabilities: [],
      menus: [{ location: 'file' as const, label: id, command: 'noop' }],
      cli,
    } as PluginManifest
  }

  it('drops conflicting subcommand cli entries from both plugins; leaves menus intact', () => {
    const a = mfest('a', [{ subcommand: 'dup', command: 'x', summary: 's' }])
    const b = mfest('b', [{ subcommand: 'dup', command: 'x', summary: 's' }])
    const reg = buildRegistry([a, b])
    expect(Object.keys(reg.byId).sort()).toEqual(['a', 'b'])
    expect(reg.byId['a'].cli ?? []).toEqual([])
    expect(reg.byId['b'].cli ?? []).toEqual([])
    expect(reg.byId['a'].menus?.length).toBe(1)
    expect(reg.byId['b'].menus?.length).toBe(1)
    expect(reg.errors.some(e => e.includes("'dup'"))).toBe(true)
  })

  it('drops a subcommand that collides with a reserved builtin', () => {
    const a = mfest('a', [{ subcommand: 'help', command: 'x', summary: 's' }])
    const reg = buildRegistry([a])
    expect(reg.byId['a'].cli ?? []).toEqual([])
    expect(reg.errors.some(e => e.includes("'help'") && e.includes('reserved'))).toBe(true)
  })

  it('drops conflicting alias entries; leaves non-conflicting cli entries from same plugin alone', () => {
    const a = mfest('a', [
      { subcommand: 'one', aliases: ['-x'], command: 'cmd', summary: 's' },
      { subcommand: 'two', command: 'cmd', summary: 's' },
    ])
    const b = mfest('b', [
      { subcommand: 'three', aliases: ['-x'], command: 'cmd', summary: 's' },
    ])
    const reg = buildRegistry([a, b])
    // 'one' has the conflicting alias; 'two' should survive.
    expect(reg.byId['a'].cli?.map(c => c.subcommand).sort()).toEqual(['two'])
    expect(reg.byId['b'].cli ?? []).toEqual([])
  })

  it('does not modify input manifests', () => {
    const a = mfest('a', [{ subcommand: 'dup', command: 'x', summary: 's' }])
    const b = mfest('b', [{ subcommand: 'dup', command: 'x', summary: 's' }])
    const aBefore = JSON.parse(JSON.stringify(a))
    buildRegistry([a, b])
    expect(a).toEqual(aBefore)
  })
})

import { isPluginActive, setActivePluginIds } from './registry'

describe('activePluginIds', () => {
  it('returns false for ids not in the active set', () => {
    setActivePluginIds(new Set())
    expect(isPluginActive('openclaw-chat')).toBe(false)
  })
  it('returns true for ids in the active set', () => {
    setActivePluginIds(new Set(['share', 'openclaw-chat']))
    expect(isPluginActive('openclaw-chat')).toBe(true)
    expect(isPluginActive('md2pdf')).toBe(false)
  })
})
