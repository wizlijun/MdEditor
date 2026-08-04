import { describe, it, expect } from 'vitest'
import { deriveStatus, listIdeas } from './status'

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
  it('keeps plain idea .md files and excludes their .proof.md counterpart', () => {
    const entries = [
      { name: '2026-08-04-a.md', is_dir: false },
      { name: '2026-08-04-a.proof.md', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.md'])
  })

  it('excludes directory entries', () => {
    const entries = [
      { name: '2026-08-04-a.md', is_dir: false },
      { name: 'subdir', is_dir: true },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.md'])
  })

  it('excludes reserved concept names (index.md/log.md)', () => {
    const entries = [
      { name: '2026-08-04-a.md', is_dir: false },
      { name: 'index.md', is_dir: false },
      { name: 'log.md', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.md'])
  })

  it('excludes non-markdown files', () => {
    const entries = [
      { name: '2026-08-04-a.md', is_dir: false },
      { name: 'notes.txt', is_dir: false },
      { name: '.DS_Store', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-a.md'])
  })

  it('sorts newest date first (descending by name)', () => {
    const entries = [
      { name: '2026-08-01-old.md', is_dir: false },
      { name: '2026-08-04-new.md', is_dir: false },
      { name: '2026-08-02-mid.md', is_dir: false },
    ]
    expect(listIdeas(entries)).toEqual(['2026-08-04-new.md', '2026-08-02-mid.md', '2026-08-01-old.md'])
  })

  it('returns an empty list for an empty or all-excluded directory', () => {
    expect(listIdeas([])).toEqual([])
    expect(listIdeas([{ name: 'index.md', is_dir: false }, { name: 'sub', is_dir: true }])).toEqual([])
  })
})
