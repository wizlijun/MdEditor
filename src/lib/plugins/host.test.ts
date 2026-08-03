import { describe, it, expect, vi } from 'vitest'
import { buildContext } from './host'
import type { PluginManifest } from './types'

const baseManifest: PluginManifest = {
  id: 'share', name: 'Share', version: '1.0.0', binary: 'bin',
  host_capabilities: ['renderer.html', 'settings.read', 'settings.write:share.records', 'toast', 'clipboard.write'],
}

describe('buildContext', () => {
  it('includes raw_content only when capability is present', async () => {
    const tab = { path: '/p/foo.md', filename: 'foo.md', extension: 'md', kind: 'markdown' as const, title: 'foo', isDirty: false, isUntitled: false, content: '# Hi' }
    const m = { ...baseManifest, host_capabilities: ['renderer.raw'] as never[] }
    const r = await buildContext(m, tab, { htmlBaker: async () => 'NEVER CALLED' })
    expect(r.context.raw_content).toBe('# Hi')
    expect(r.context.rendered_html).toBeUndefined()
  })

  it('calls htmlBaker only when renderer.html declared', async () => {
    const tab = { path: '/p/foo.md', filename: 'foo.md', extension: 'md', kind: 'markdown' as const, title: 'foo', isDirty: false, isUntitled: false, content: '# Hi' }
    const baker = vi.fn().mockResolvedValue('<html>x</html>')

    const m1 = { ...baseManifest, host_capabilities: ['toast'] as never[] }
    await buildContext(m1, tab, { htmlBaker: baker })
    expect(baker).not.toHaveBeenCalled()

    const m2 = { ...baseManifest, host_capabilities: ['renderer.html'] as never[] }
    const r = await buildContext(m2, tab, { htmlBaker: baker })
    expect(baker).toHaveBeenCalledOnce()
    expect(r.context.rendered_html).toBe('<html>x</html>')
  })

  it('omits settings field when settings.read is absent', async () => {
    const tab = { path: '/p/foo.md', filename: 'foo.md', extension: 'md', kind: 'markdown' as const, title: 'foo', isDirty: false, isUntitled: false, content: '' }
    const m = { ...baseManifest, host_capabilities: ['toast'] as never[] }
    const r = await buildContext(m, tab, { htmlBaker: async () => '', settingsReader: () => ({ 'share.x': 1 }) })
    expect(r.settings).toBeUndefined()
  })

  it('includes scoped settings when settings.read declared', async () => {
    const tab = { path: '/p/foo.md', filename: 'foo.md', extension: 'md', kind: 'markdown' as const, title: 'foo', isDirty: false, isUntitled: false, content: '' }
    const r = await buildContext(baseManifest, tab,
      { htmlBaker: async () => '<x/>', settingsReader: () => ({ 'share.baseUrl': 'https://x' }) })
    expect(r.settings).toEqual({ 'share.baseUrl': 'https://x' })
  })

  // A plugin's `cli_str`/`cli_flag` helper (e.g. ebook-import's or
  // roam-import's `plugin.rs`) probes `context.cli.args`/`context.cli.flags`
  // first. If `buildContext` doesn't forward what the CLI runner parsed,
  // every `--flag` a CLI user typed is silently dropped on the floor and the
  // plugin falls back to its own defaults with no error — this is exactly
  // the bug Task 10's live E2E run failed to catch (it only exercised the
  // default date).
  it('omits context.cli when the caller passes no cli opt (GUI-triggered commands)', async () => {
    const tab = { path: '/p/foo.md', filename: 'foo.md', extension: 'md', kind: 'markdown' as const, title: 'foo', isDirty: false, isUntitled: false, content: '' }
    const m = { ...baseManifest, host_capabilities: [] as never[] }
    const r = await buildContext(m, tab, {})
    expect(r.context.cli).toBeUndefined()
  })

  it('forwards cli.args and cli.flags verbatim when the caller passes them', async () => {
    const tab = { path: '', filename: null, extension: null, kind: 'markdown' as const, title: '', isDirty: false, isUntitled: true, content: '' }
    const m = { ...baseManifest, host_capabilities: [] as never[] }
    const r = await buildContext(m, tab, {
      cli: { args: {}, flags: { date: '2026-08-02', graph: 'bruce' } },
    })
    expect(r.context.cli).toEqual({ args: {}, flags: { date: '2026-08-02', graph: 'bruce' } })
  })

  it('forwards the positional file under cli.args.file', async () => {
    const tab = { path: '/p/book.epub', filename: 'book.epub', extension: 'epub', kind: 'markdown' as const, title: 'book', isDirty: false, isUntitled: false, content: '' }
    const m = { ...baseManifest, host_capabilities: [] as never[] }
    const r = await buildContext(m, tab, {
      cli: { args: { file: '/p/book.epub' }, flags: { ocr: true } },
    })
    expect(r.context.cli).toEqual({ args: { file: '/p/book.epub' }, flags: { ocr: true } })
  })
})
