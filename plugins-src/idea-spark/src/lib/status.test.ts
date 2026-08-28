import { describe, it, expect } from 'vitest'
import { ICONS } from './icons'
import { deriveStatus, listIdeas, STATUS_KEY, STATUS_MARK, type IdeaStatus } from './status'

describe('deriveStatus', () => {
  const name = '2026-08-04-my-idea.md'

  it('is "done" when the matching .proof.md exists, regardless of pending/failed', () => {
    const files = new Set([name, '2026-08-04-my-idea.proof.md'])
    expect(deriveStatus(name, files, { [name]: 'run-1' }, new Set([name]))).toBe('done')
  })

  it('is "running" when the idea has an active run and no proof file yet', () => {
    const files = new Set([name])
    expect(deriveStatus(name, files, { [name]: 'run-1' }, new Set())).toBe('running')
  })

  it('is "failed" when marked failed and neither proof nor a pending run exists', () => {
    const files = new Set([name])
    expect(deriveStatus(name, files, {}, new Set([name]))).toBe('failed')
  })

  it('is "draft" when it is just a bare idea file with no run history', () => {
    const files = new Set([name])
    expect(deriveStatus(name, files, {}, new Set())).toBe('draft')
  })

  it('done takes priority over running (stale pending entry the watcher has not cleared yet)', () => {
    const files = new Set([name, '2026-08-04-my-idea.proof.md'])
    expect(deriveStatus(name, files, { [name]: 'run-1' }, new Set())).toBe('done')
  })

  it('running takes priority over failed', () => {
    const files = new Set([name])
    expect(deriveStatus(name, files, { [name]: 'run-1' }, new Set([name]))).toBe('running')
  })
})

describe('listIdeas', () => {
  it('keeps only files whose names end exactly in .idea.md', () => {
    const entries = [
      { name: '2026-08-04-a.idea.md', is_dir: false },
      { name: '2026-08-04-a.md', is_dir: false },
      { name: '2026-08-04-a-idea.md', is_dir: false },
      { name: '2026-08-04-a.idea.proof.md', is_dir: false },
      { name: '2026-08-04-a.IDEA.md', is_dir: false },
      { name: '2026-08-04-a.idea.md.bak', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.idea.md'])
  })

  it('excludes directory entries', () => {
    const entries = [
      { name: '2026-08-04-a.idea.md', is_dir: false },
      { name: 'subdir.idea.md', is_dir: true },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.idea.md'])
  })

  it('excludes reserved concept names (index.md/log.md)', () => {
    const entries = [
      { name: '2026-08-04-a.idea.md', is_dir: false },
      { name: 'index.md', is_dir: false },
      { name: 'log.md', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.idea.md'])
  })

  it('excludes non-markdown files', () => {
    const entries = [
      { name: '2026-08-04-a.idea.md', is_dir: false },
      { name: 'notes.txt', is_dir: false },
      { name: '.DS_Store', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.idea.md'])
  })

  it('sorts newest date first (descending by name)', () => {
    const entries = [
      { name: '2026-08-01-old.idea.md', is_dir: false },
      { name: '2026-08-04-new.idea.md', is_dir: false },
      { name: '2026-08-02-mid.idea.md', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual([
      '2026-08-04-new.idea.md',
      '2026-08-02-mid.idea.md',
      '2026-08-01-old.idea.md',
    ])
  })

  it('returns an empty list for an empty or all-excluded directory', () => {
    expect(listIdeas([])).toEqual([])
    expect(listIdeas([{ name: 'index.md', is_dir: false }, { name: 'sub', is_dir: true }])).toEqual([])
  })
})

describe('STATUS_MARK', () => {
  const ALL: IdeaStatus[] = ['draft', 'running', 'done', 'failed']

  // THE point of this block. `✦` is not decoration and not an icon: across
  // note.md it means "written by AI" (and `●` means "written by you") —
  // CLAUDE.md, belief 3. An argued idea wears the same `✦` its proof document
  // carries. Replacing it with a generic check mark or a "done" icon would
  // silently drop that meaning, so it is pinned here rather than left to a
  // comment and a reviewer's attention.
  it('marks an argued idea with the ✦ that means "written by AI"', () => {
    expect(STATUS_MARK.done).toEqual({ kind: 'glyph', text: '✦' })
  })

  it('offers no icon that could be swapped in for that ✦', () => {
    // The other half of the guard above: an icon named `done`/`argued` would
    // be an invitation. Deleting this test is a decision; failing to notice
    // the icon exists is an accident.
    for (const forbidden of ['done', 'argued', 'success', 'check']) {
      expect(Object.keys(ICONS)).not.toContain(forbidden)
    }
  })

  it('leaves a draft unmarked — most rows are drafts and a badge on each is noise', () => {
    expect(STATUS_MARK.draft).toBeNull()
  })

  it('draws the two machine states as icons, so they follow currentColor', () => {
    // Emoji/glyphs could not be tinted; `failed` in particular has to take the
    // warning red from its row's CSS.
    expect(STATUS_MARK.running).toEqual({ kind: 'icon', icon: 'running' })
    expect(STATUS_MARK.failed).toEqual({ kind: 'icon', icon: 'failed' })
  })

  it('names an icon that actually exists for every icon mark', () => {
    for (const status of ALL) {
      const mark = STATUS_MARK[status]
      if (mark?.kind === 'icon') expect(ICONS).toHaveProperty(mark.icon)
    }
  })

  it('covers every status, in both tables', () => {
    // `deriveStatus` can return any of these; a missing entry is an undefined
    // read in the template, not a type error, once `IdeaStatus` grows.
    expect(Object.keys(STATUS_MARK).sort()).toEqual([...ALL].sort())
    expect(Object.keys(STATUS_KEY).sort()).toEqual([...ALL].sort())
  })
})
