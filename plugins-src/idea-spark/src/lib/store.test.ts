// Tests for the pure state transitions in store.svelte.ts.
//
// Only the *pure* half is covered here: transitions take an explicit store
// object (created by `createStore()`), never the module-level `$state`
// singleton, so a test can build an isolated store, apply a transition and
// assert on the result without a Svelte component tree or a host bridge.
// The async actions (boot/reload/save/...) are bridge IO and are exercised
// by hand in the window — see the task report's manual-verification list.
import { describe, it, expect } from 'vitest'
import {
  applyRunDone,
  bodyOf,
  createStore,
  frontmatterOf,
  displayName,
  ideaTemplate,
  markPending,
  relPath,
  setIdeaDir,
  statusOf,
  type SparkStore,
} from './store.svelte'

/** A store holding one saved idea (`inbox/ideas/a.md`) and nothing else. */
function storeWithIdea(): SparkStore {
  const s = createStore()
  s.files = ['inbox/ideas/a.md']
  s.docs = ['a.md']
  return s
}

describe('applyRunDone', () => {
  it('success marks the idea done, records the result and raises celebrate', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')

    const status = applyRunDone(s, {
      run_id: 'run-1',
      status: 'success',
      open_path: 'inbox/ideas/a.proof.md',
    })

    expect(status).toBe('done')
    expect(s.pending).toEqual({})
    expect(s.files).toContain('inbox/ideas/a.proof.md')
    expect(statusOf(s, 'a.md')).toBe('done')
    expect(s.celebrate).toBe(true)
    expect(s.lastResult).toBe('inbox/ideas/a.proof.md')
  })

  it('success without open_path derives the proof path from the idea path', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')

    applyRunDone(s, { run_id: 'run-1', status: 'success' })

    expect(s.files).toContain('inbox/ideas/a.proof.md')
    expect(s.lastResult).toBe('inbox/ideas/a.proof.md')
    expect(statusOf(s, 'a.md')).toBe('done')
  })

  it('success does not duplicate a proof file the listing already knows about', () => {
    const s = storeWithIdea()
    s.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md']
    markPending(s, 'inbox/ideas/a.md', 'run-1')

    applyRunDone(s, { run_id: 'run-1', status: 'success', open_path: 'inbox/ideas/a.proof.md' })

    expect(s.files.filter((f) => f === 'inbox/ideas/a.proof.md')).toHaveLength(1)
  })

  it('success clears a previous failure for the same idea (re-delegation succeeded)', () => {
    const s = storeWithIdea()
    s.failed = ['inbox/ideas/a.md']
    markPending(s, 'inbox/ideas/a.md', 'run-2')

    applyRunDone(s, { run_id: 'run-2', status: 'success', open_path: 'inbox/ideas/a.proof.md' })

    expect(s.failed).toEqual([])
    expect(statusOf(s, 'a.md')).toBe('done')
  })

  for (const status of ['error', 'lost', 'timeout', 'cancelled']) {
    it(`'${status}' marks the idea failed and never celebrates`, () => {
      const s = storeWithIdea()
      markPending(s, 'inbox/ideas/a.md', 'run-1')

      const result = applyRunDone(s, { run_id: 'run-1', status })

      expect(result).toBe('failed')
      expect(s.pending).toEqual({})
      expect(s.failed).toEqual(['inbox/ideas/a.md'])
      expect(statusOf(s, 'a.md')).toBe('failed')
      expect(s.celebrate).toBe(false)
      expect(s.lastResult).toBeNull()
      expect(s.files).not.toContain('inbox/ideas/a.proof.md')
    })
  }

  it('a failure does not record the same idea twice', () => {
    const s = storeWithIdea()
    s.failed = ['inbox/ideas/a.md']
    markPending(s, 'inbox/ideas/a.md', 'run-2')

    applyRunDone(s, { run_id: 'run-2', status: 'error' })

    expect(s.failed).toEqual(['inbox/ideas/a.md'])
  })

  it('an unknown run_id is a no-op (a stale push from another window/session)', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')

    const result = applyRunDone(s, { run_id: 'other-run', status: 'success', open_path: 'x.proof.md' })

    expect(result).toBeNull()
    expect(s.pending).toEqual({ 'inbox/ideas/a.md': 'run-1' })
    expect(s.failed).toEqual([])
    expect(s.celebrate).toBe(false)
    expect(s.lastResult).toBeNull()
    expect(s.files).toEqual(['inbox/ideas/a.md'])
  })
})

describe('markPending', () => {
  it('registers the run and flips the idea to running', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')
    expect(s.pending).toEqual({ 'inbox/ideas/a.md': 'run-1' })
    expect(statusOf(s, 'a.md')).toBe('running')
  })

  it('clears a previous failure so a retry does not render as failed', () => {
    const s = storeWithIdea()
    s.failed = ['inbox/ideas/a.md']
    markPending(s, 'inbox/ideas/a.md', 'run-2')
    expect(s.failed).toEqual([])
    expect(statusOf(s, 'a.md')).toBe('running')
  })
})

describe('statusOf', () => {
  it('defaults to draft for a plain idea file', () => {
    expect(statusOf(storeWithIdea(), 'a.md')).toBe('draft')
  })

  it('is done when the proof sidecar is in the listing', () => {
    const s = storeWithIdea()
    s.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md']
    expect(statusOf(s, 'a.md')).toBe('done')
  })
})

describe('setIdeaDir', () => {
  it('accepts a plain vault-relative directory', () => {
    const s = createStore()
    expect(setIdeaDir(s, 'notes/sparks')).toBe(true)
    expect(s.ideaDir).toBe('notes/sparks')
  })

  it('trims surrounding whitespace and trailing slashes', () => {
    const s = createStore()
    expect(setIdeaDir(s, '  notes/sparks/  ')).toBe(true)
    expect(s.ideaDir).toBe('notes/sparks')
  })

  it.each(['', '   ', '/', '///'])('rejects the empty directory %o', (dir) => {
    const s = createStore()
    const before = s.ideaDir
    expect(setIdeaDir(s, dir)).toBe(false)
    expect(s.ideaDir).toBe(before)
  })

  it.each(['/abs/path', '/inbox/ideas'])('rejects the absolute path %o', (dir) => {
    const s = createStore()
    const before = s.ideaDir
    expect(setIdeaDir(s, dir)).toBe(false)
    expect(s.ideaDir).toBe(before)
  })

  it.each(['..', 'a/../b', '../escape', 'inbox/..'])('rejects the traversing path %o', (dir) => {
    const s = createStore()
    const before = s.ideaDir
    expect(setIdeaDir(s, dir)).toBe(false)
    expect(s.ideaDir).toBe(before)
  })
})

describe('relPath', () => {
  it('joins the idea directory and the file name', () => {
    const s = createStore()
    setIdeaDir(s, 'notes/sparks')
    expect(relPath(s, 'a.md')).toBe('notes/sparks/a.md')
  })
})

describe('ideaTemplate', () => {
  it('opens with the localized H1 and carries the four sections in order', () => {
    const tpl = ideaTemplate()
    expect(tpl.startsWith('# New idea\n')).toBe(true)
    const headings = tpl.split('\n').filter((l) => l.startsWith('## '))
    expect(headings).toEqual(['## Domain', '## Transfer', '## Resources', '## Outcome'])
  })
})

describe('bodyOf', () => {
  it('strips OKF frontmatter and the blank lines that follow it', () => {
    expect(bodyOf('---\ntype: Idea\ncreated: x\n---\n\n# Title\n\nbody')).toBe('# Title\n\nbody')
  })

  it('leaves a document without frontmatter alone', () => {
    expect(bodyOf('# Title\n\n---\n\nmore')).toBe('# Title\n\n---\n\nmore')
  })
})

describe('frontmatterOf', () => {
  it('returns the block between the fences, without the fences', () => {
    expect(frontmatterOf('---\ntype: Idea\ncreated: x\n---\n\n# Title')).toBe('type: Idea\ncreated: x')
  })

  it('reads a CRLF file without dragging carriage returns along', () => {
    expect(frontmatterOf('---\r\ntype: Idea\r\n---\r\n\r\n# Title')).toBe('type: Idea')
  })

  it('is null for a document with no frontmatter at all', () => {
    expect(frontmatterOf('# Title\n\n---\n\nmore')).toBeNull()
  })

  it('is null when the opening fence is never closed (do not guess)', () => {
    expect(frontmatterOf('---\ntype: Idea\n\n# Title')).toBeNull()
  })

  it('is an empty string for an empty frontmatter block', () => {
    expect(frontmatterOf('---\n---\nbody')).toBe('')
  })
})

describe('displayName', () => {
  it('drops the date prefix and the extension', () => {
    expect(displayName('2026-08-04-my-idea.md')).toBe('my-idea')
  })

  it('keeps a name that has no date prefix', () => {
    expect(displayName('my-idea.md')).toBe('my-idea')
  })

  it('keeps the date when it is all there is', () => {
    expect(displayName('2026-08-04.md')).toBe('2026-08-04')
  })
})
