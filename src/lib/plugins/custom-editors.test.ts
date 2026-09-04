import { describe, it, expect, vi, afterEach } from 'vitest'
import { buildCustomEditorRegistry, customEditorFor } from './custom-editors'
import type { PluginManifest } from './types'

const mf = (over: Partial<PluginManifest> = {}): PluginManifest => ({
  id: 'notemd.base',
  name: 'Base',
  version: '1.0.0',
  binary: '',
  host_capabilities: [],
  ...over,
})

const baseEditor = {
  id: 'base-table',
  file_extensions: ['.base'],
  entry: 'editor.html',
}

afterEach(() => vi.restoreAllMocks())

describe('buildCustomEditorRegistry', () => {
  it('maps a single extension to its editor', () => {
    const reg = buildCustomEditorRegistry([mf({ custom_editors: [baseEditor] })])
    expect(reg.get('base')).toEqual({
      pluginId: 'notemd.base',
      editorId: 'base-table',
      entry: 'editor.html',
    })
  })

  it('maps every extension of a multi-ext editor (dot optional, case-insensitive)', () => {
    const reg = buildCustomEditorRegistry([
      mf({ custom_editors: [{ id: 'e', file_extensions: ['.Base', 'DB', '.tbl'], entry: 'e.html' }] }),
    ])
    expect(reg.size).toBe(3)
    for (const ext of ['base', 'db', 'tbl']) {
      expect(reg.get(ext)).toEqual({ pluginId: 'notemd.base', editorId: 'e', entry: 'e.html' })
    }
  })

  it('returns an empty map when no plugin has custom_editors', () => {
    expect(buildCustomEditorRegistry([mf(), mf({ id: 'p.other' })]).size).toBe(0)
  })

  it('is empty for v1-only hosts (no custom_editors field)', () => {
    const v1: PluginManifest = mf({ id: 'share', menus: [] })
    expect(buildCustomEditorRegistry([v1]).size).toBe(0)
  })

  it('first plugin wins on a colliding extension and warns', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const reg = buildCustomEditorRegistry([
      mf({ id: 'a.one', custom_editors: [{ id: 'x', file_extensions: ['.z'], entry: 'a.html' }] }),
      mf({ id: 'b.two', custom_editors: [{ id: 'y', file_extensions: ['.z'], entry: 'b.html' }] }),
    ])
    expect(reg.get('z')!.pluginId).toBe('a.one')
    expect(warn).toHaveBeenCalledOnce()
  })

  it('skips malformed editor entries', () => {
    const reg = buildCustomEditorRegistry([
      mf({
        custom_editors: [
          { id: '', file_extensions: ['.a'], entry: 'e.html' } as never, // no id
          { id: 'ok', file_extensions: [], entry: 'e.html' }, // no extensions
          { id: 'ok2', file_extensions: ['.b'], entry: 'ok.html' }, // valid
        ],
      }),
    ])
    expect(reg.size).toBe(1)
    expect(reg.get('b')!.editorId).toBe('ok2')
  })
})

describe('customEditorFor', () => {
  const manifests = [mf({ custom_editors: [baseEditor] })]

  it('finds a registered editor by extension (dot or bare)', () => {
    expect(customEditorFor('.base', manifests)!.editorId).toBe('base-table')
    expect(customEditorFor('base', manifests)!.editorId).toBe('base-table')
    expect(customEditorFor('.BASE', manifests)!.editorId).toBe('base-table')
  })

  it('returns null for an unregistered extension', () => {
    expect(customEditorFor('.md', manifests)).toBeNull()
    expect(customEditorFor('', manifests)).toBeNull()
  })
})

// ── CEF fixture manifest shape ──────────────────────────────────────────────
// Concrete regression anchor: the EXACT custom_editors shape from
// plugins-src/custom-editor-fixture/manifest.v2.json is routed correctly.
describe('cef-fixture manifest shape', () => {
  const cefManifest: PluginManifest = mf({
    id: 'notemd.cef-fixture',
    name: 'Custom Editor Fixture',
    version: '1.0.0',
    custom_editors: [
      { id: 'cef', file_extensions: ['.cef'], entry: 'editor.html' },
    ],
  })

  it('registers .cef → editor.html under notemd.cef-fixture', () => {
    const ref = customEditorFor('.cef', [cefManifest])
    expect(ref).not.toBeNull()
    expect(ref!.pluginId).toBe('notemd.cef-fixture')
    expect(ref!.editorId).toBe('cef')
    expect(ref!.entry).toBe('editor.html')
  })

  it('does not register unrelated extensions', () => {
    expect(customEditorFor('.md', [cefManifest])).toBeNull()
    expect(customEditorFor('.base', [cefManifest])).toBeNull()
  })

  it('cef editor does not collide with base when both loaded', () => {
    const baseManifest = mf({ id: 'notemd.base', custom_editors: [baseEditor] })
    const reg = buildCustomEditorRegistry([cefManifest, baseManifest])
    expect(reg.get('cef')!.pluginId).toBe('notemd.cef-fixture')
    expect(reg.get('base')!.pluginId).toBe('notemd.base')
  })

  it('refuses reserved core extensions (md/txt/html/canvas) but keeps custom ones', () => {
    const hijacker = mf({
      id: 'evil.plugin',
      custom_editors: [{ id: 'h', file_extensions: ['.md', 'markdown', 'txt', 'html', '.canvas', '.custom'], entry: 'h.html' }],
    })
    const reg = buildCustomEditorRegistry([hijacker])
    expect(reg.has('md')).toBe(false)
    expect(reg.has('markdown')).toBe(false)
    expect(reg.has('txt')).toBe(false)
    expect(reg.has('html')).toBe(false)
    expect(reg.has('canvas')).toBe(false)
    // Non-reserved extensions still register.
    expect(reg.get('custom')!.pluginId).toBe('evil.plugin')
    expect(customEditorFor('.md', [hijacker])).toBeNull()
    expect(customEditorFor('.canvas', [hijacker])).toBeNull()
  })
})
