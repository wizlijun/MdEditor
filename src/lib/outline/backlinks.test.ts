// src/lib/outline/backlinks.test.ts
import { describe, it, expect, afterEach } from 'vitest'
import { createIndex, indexFileContent, removeFileFromIndex, backlinksFor, pageNameOf, pageCandidates, resolveTarget, detectNameCollisions, isWikiPagePath, classifyWatchPaths } from './backlinks'
import { setBlockedWikilinks } from '../wikilink/blocklist'

function idxWith(files: Record<string, string>) {
  const idx = createIndex()
  for (const [p, c] of Object.entries(files)) indexFileContent(idx, p, c)
  return idx
}

describe('resolveTarget', () => {
  it('resolves plain .md by filename (case-insensitive)', () => {
    const idx = idxWith({ '/v/Foo.md': 'x' })
    expect(resolveTarget(idx, 'foo')).toBe('/v/Foo.md')
  })
  it('standalone .note.md IS a valid target (wiki page)', () => {
    const idx = idxWith({ '/v/wikipage/wiki.note.md': '- x' })
    expect(resolveTarget(idx, 'wiki')).toBe('/v/wikipage/wiki.note.md')
  })
  it('companion .note.md is NEVER a target; main doc wins', () => {
    const idx = idxWith({ '/v/a.md': 'x', '/v/a.note.md': '- anno' })
    expect(resolveTarget(idx, 'a')).toBe('/v/a.md')
  })
  it('main doc beats a同名 standalone note in another dir', () => {
    const idx = idxWith({ '/v/sub/x.md': 'x', '/v/wikipage/x.note.md': '- x' })
    expect(resolveTarget(idx, 'x')).toBe('/v/sub/x.md')
  })
  it('null when nothing matches', () => {
    expect(resolveTarget(idxWith({}), 'nope')).toBeNull()
  })
})

describe('detectNameCollisions', () => {
  it('reports same page name in different dirs', () => {
    const idx = idxWith({ '/v/a/x.md': '1', '/v/b/x.md': '2' })
    const m = detectNameCollisions(idx)
    expect(m.get('x')).toEqual(expect.arrayContaining(['/v/a/x.md', '/v/b/x.md']))
  })
  it('companion pair is NOT a collision', () => {
    const idx = idxWith({ '/v/a.md': '1', '/v/a.note.md': '- x' })
    expect(detectNameCollisions(idx).size).toBe(0)
  })
  it('standalone note vs md with same name IS a collision', () => {
    const idx = idxWith({ '/v/sub/x.md': '1', '/v/wikipage/x.note.md': '- x' })
    expect(detectNameCollisions(idx).get('x')).toHaveLength(2)
  })
})

describe('pageNameOf', () => {
  it('strips extension and .notes suffix', () => {
    expect(pageNameOf('/dir/Foo.md')).toBe('Foo')
    expect(pageNameOf('/dir/Foo.notes.md')).toBe('Foo')
    expect(pageNameOf('/dir/a.b.md')).toBe('a.b')
  })
  it('strips .note.md, legacy .notes.md and plain .md', () => {
    expect(pageNameOf('/v/foo.note.md')).toBe('foo')
    expect(pageNameOf('/v/foo.notes.md')).toBe('foo')
    expect(pageNameOf('/v/foo.md')).toBe('foo')
  })
})

describe('index', () => {
  afterEach(() => setBlockedWikilinks([]))
  it('collects [[links]] and #tags with node text and line', () => {
    const idx = createIndex()
    indexFileContent(idx, '/d/one.notes.md', '- see [[Target]] here\n- #Target tagged\n- nothing\n')
    expect(backlinksFor(idx, 'target')).toEqual([
      { file: '/d/one.notes.md', text: 'see [[Target]] here', line: 1 },
      { file: '/d/one.notes.md', text: '#Target tagged', line: 2 },
    ])
  })
  it('indexes [[links]] wrapped in emphasis (**, ^^, …)', () => {
    const idx = createIndex()
    indexFileContent(idx, '/d/e.notes.md', '- bold **[[Target]]** here\n- ^^see [[Target]] hi^^\n')
    expect(backlinksFor(idx, 'target')).toEqual([
      { file: '/d/e.notes.md', text: 'bold **[[Target]]** here', line: 1 },
      { file: '/d/e.notes.md', text: '^^see [[Target]] hi^^', line: 2 },
    ])
  })
  it('re-indexing a file replaces its old entries', () => {
    const idx = createIndex()
    indexFileContent(idx, '/d/a.md', 'x [[T]]\n')
    indexFileContent(idx, '/d/a.md', 'no links now\n')
    expect(backlinksFor(idx, 't')).toEqual([])
  })
  it('removeFileFromIndex drops entries', () => {
    const idx = createIndex()
    indexFileContent(idx, '/d/a.md', '[[T]]\n')
    removeFileFromIndex(idx, '/d/a.md')
    expect(backlinksFor(idx, 't')).toEqual([])
  })
  it('pageCandidates lists indexed file pages, unique', () => {
    const idx = createIndex()
    indexFileContent(idx, '/d/Alpha.md', 'x\n')
    indexFileContent(idx, '/d/Alpha.notes.md', 'y\n')
    indexFileContent(idx, '/d/Beta.md', 'z\n')
    expect(pageCandidates(idx).sort()).toEqual(['Alpha', 'Beta'])
  })
  it('does not index blocklisted wikilinks', () => {
    setBlockedWikilinks(['wikilink'])
    const idx = createIndex()
    indexFileContent(idx, '/d/a.notes.md', '- [[wikilink]] and [[Real]]\n')
    expect(backlinksFor(idx, 'wikilink')).toEqual([])
    expect(backlinksFor(idx, 'real')).toHaveLength(1)
  })
})

describe('scoped index (wikipage/dailynote only)', () => {
  const SCOPE = { root: '/v', dirs: ['wikipage', 'dailynote'] }
  function scopedIdx(files: Record<string, string>) {
    const idx = createIndex(SCOPE)
    for (const [p, c] of Object.entries(files)) indexFileContent(idx, p, c)
    return idx
  }

  it('wiki page beats a同名 stray .md; stray is unresolvable', () => {
    const idx = scopedIdx({ '/v/sub/x.md': 'x', '/v/wikipage/x.note.md': '- x' })
    expect(resolveTarget(idx, 'x')).toBe('/v/wikipage/x.note.md')
  })
  it('two stray .md with same name are NOT a collision', () => {
    const idx = scopedIdx({ '/v/a/foo.md': '1', '/v/b/foo.md': '2' })
    expect(detectNameCollisions(idx).size).toBe(0)
  })
  it('two wiki pages with same name ARE a collision', () => {
    const idx = scopedIdx({
      '/v/wikipage/foo.note.md': '- 1',
      '/v/wikipage/sub/foo.note.md': '- 2',
    })
    expect(detectNameCollisions(idx).get('foo')).toHaveLength(2)
  })
  it('nested dailynote page is resolvable (recursive)', () => {
    const idx = scopedIdx({ '/v/dailynote/2026/2026-07-11.note.md': '- d' })
    expect(resolveTarget(idx, '2026-07-11')).toBe('/v/dailynote/2026/2026-07-11.note.md')
  })
  it('stray doc linking a wiki page is still a backlink source', () => {
    const idx = scopedIdx({
      '/v/sub/note.md': '- see [[Wiki]] here\n',
      '/v/wikipage/wiki.note.md': '- x',
    })
    expect(resolveTarget(idx, 'stray-none')).toBeNull()
    expect(backlinksFor(idx, 'wiki')).toEqual([
      { file: '/v/sub/note.md', text: 'see [[Wiki]] here', line: 1 },
    ])
    expect(pageCandidates(idx)).toEqual(['wiki'])
  })
  // 增量重扫(file-watcher 走 indexFileContent)须沿用 scope:散落文件重扫后
  // 仍只更新 byTarget,永不进 filePages。
  it('re-indexing a stray file honors scope (byTarget updates, filePages stays empty)', () => {
    const idx = scopedIdx({ '/v/sub/note.md': '- old\n' })
    indexFileContent(idx, '/v/sub/note.md', '- now links [[Wiki]]\n')
    expect(resolveTarget(idx, 'note')).toBeNull()
    expect(pageCandidates(idx)).toEqual([])
    expect(backlinksFor(idx, 'wiki')).toEqual([
      { file: '/v/sub/note.md', text: 'now links [[Wiki]]', line: 1 },
    ])
  })
  it('honors a custom (renamed) scope dir', () => {
    const idx = createIndex({ root: '/v', dirs: ['notes'] })
    indexFileContent(idx, '/v/notes/x.note.md', '- x')
    indexFileContent(idx, '/v/wikipage/y.note.md', '- y')
    expect(resolveTarget(idx, 'x')).toBe('/v/notes/x.note.md')
    expect(resolveTarget(idx, 'y')).toBeNull()
  })
})

describe('isWikiPagePath', () => {
  const scope = { root: '/v', dirs: ['wikipage', 'dailynote'] }
  it('true for .md directly under a scope dir', () => {
    expect(isWikiPagePath(scope, '/v/wikipage/x.note.md')).toBe(true)
  })
  it('true for .md nested deeper under a scope dir (recursive)', () => {
    expect(isWikiPagePath(scope, '/v/dailynote/2026/2026-07-11.note.md')).toBe(true)
  })
  it('false for .md outside scope dirs', () => {
    expect(isWikiPagePath(scope, '/v/sub/x.md')).toBe(false)
  })
  it('false for a file sitting at root without a scope dir', () => {
    expect(isWikiPagePath(scope, '/v/x.md')).toBe(false)
  })
  it('false for non-.md even under a scope dir', () => {
    expect(isWikiPagePath(scope, '/v/wikipage/x.txt')).toBe(false)
  })
  it('false when path is outside root', () => {
    expect(isWikiPagePath(scope, '/other/wikipage/x.md')).toBe(false)
  })
  it('null scope → every .md is a page (backward compat)', () => {
    expect(isWikiPagePath(null, '/anywhere/x.md')).toBe(true)
    expect(isWikiPagePath(null, '/anywhere/x.txt')).toBe(false)
  })
  it('tolerates trailing slash on root', () => {
    expect(isWikiPagePath({ root: '/v/', dirs: ['wikipage'] }, '/v/wikipage/x.md')).toBe(true)
  })
})

describe('classifyWatchPaths', () => {
  it('separates note paths from a directory-level change', () => {
    const r = classifyWatchPaths([
      '/v/wikipage/Foo.note.md',
      '/v/ssot/books/Paper Bushcraft',   // 目录改名:事件只报目录自身
    ])
    expect(r.notes).toEqual(['/v/wikipage/Foo.note.md'])
    expect(r.dirChange).toBe(true)
  })

  it('plain files and dot-dirs are neither notes nor a dir change', () => {
    const r = classifyWatchPaths([
      '/v/notes/a.md',            // 纯 .md 不入反链索引(设计如此)
      '/v/images/cover.png',
      '/v/.git/objects',          // 点目录排除
    ])
    expect(r.notes).toEqual([])
    expect(r.dirChange).toBe(false)
  })

  it('a dotted directory name is missed by design, not crashed on', () => {
    expect(classifyWatchPaths(['/v/notes.v2']).dirChange).toBe(false)
  })
})
