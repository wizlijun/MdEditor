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
  changeIdeaDir,
  clockTime,
  createStore,
  frontmatterOf,
  displayName,
  ideaDocText,
  ideaTemplate,
  isBlank,
  markEdited,
  markPending,
  needsSaveBefore,
  nextFileName,
  rebaseline,
  relPath,
  showInEditor,
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

  it('folds the CONVENTIONAL proof path into the listing even when open_path differs', () => {
    // `deriveStatus` and `openResult` both key off `<base>.proof.md`. If an
    // off-convention `open_path` were the thing folded into `files`, this
    // function would return 'done' while the row still rendered as a draft
    // with no way to open the result.
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')

    const status = applyRunDone(s, {
      run_id: 'run-1',
      status: 'success',
      open_path: 'somewhere/else/report.md',
    })

    expect(status).toBe('done')
    expect(s.files).toContain('inbox/ideas/a.proof.md')
    expect(s.files).not.toContain('somewhere/else/report.md')
    expect(statusOf(s, 'a.md')).toBe('done')
    // The artifact the run actually produced is still remembered verbatim.
    expect(s.lastResult).toBe('somewhere/else/report.md')
  })

  it('bumps celebrateSeq on every success so a second burst is not cut short', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')
    applyRunDone(s, { run_id: 'run-1', status: 'success' })
    const first = s.celebrateSeq

    markPending(s, 'inbox/ideas/a.md', 'run-2')
    applyRunDone(s, { run_id: 'run-2', status: 'success' })

    expect(s.celebrateSeq).toBe(first + 1)
  })

  it('leaves celebrateSeq alone on failure', () => {
    const s = storeWithIdea()
    markPending(s, 'inbox/ideas/a.md', 'run-1')
    applyRunDone(s, { run_id: 'run-1', status: 'error' })
    expect(s.celebrateSeq).toBe(0)
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

// The save path's naming/serialization decisions, extracted from `saveIdea` so
// the promises the brief makes about them ("first save names the file, later
// saves overwrite it", "re-saving preserves created and unknown keys") are
// pinned by tests instead of only by the bridge-driven action around them.
describe('nextFileName', () => {
  it('names a first save by the creation timestamp, deduped against the directory', () => {
    const s = createStore()
    const at = new Date(2026, 7, 4, 19, 42).toISOString()
    s.files = ['inbox/ideas/2026-08-04-1942-idea.md']
    expect(nextFileName(s, '# my idea\n\nbody', at)).toBe('2026-08-04-1942-idea-2.md')
  })

  it('dedupes against non-idea files in the directory too', () => {
    const s = createStore()
    const at = new Date(2026, 7, 4, 19, 42).toISOString()
    // An orphaned sidecar from a *different* minute does not occupy this
    // minute's name...
    s.files = ['inbox/ideas/2026-08-04-1941-idea.proof.md']
    expect(nextFileName(s, '# a', at)).toBe('2026-08-04-1942-idea.md')
    // ...but an exact-name collision — idea or sidecar alike — does.
    s.files = ['inbox/ideas/2026-08-04-1942-idea.md', 'inbox/ideas/2026-08-04-1942-idea.proof.md']
    expect(nextFileName(s, '# a', at)).toBe('2026-08-04-1942-idea-2.md')
  })

  it('names a never-saved idea by timestamp and keeps the name afterwards', () => {
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    const at = new Date(2026, 7, 4, 19, 42).toISOString()
    expect(nextFileName(s, '# 随便什么标题', at)).toBe('2026-08-04-1942-idea.md')
    s.current = '2026-08-04-1942-idea.md'
    expect(nextFileName(s, '# 改了标题', new Date(2026, 7, 5, 8, 0).toISOString())).toBe(
      '2026-08-04-1942-idea.md',
    )
  })
})

describe('ideaDocText', () => {
  it('stamps fresh OKF frontmatter for an idea that has never been saved', () => {
    const s = createStore()
    const out = ideaDocText(s, '# Title', '2026-08-04T10:00:00Z')
    expect(out).toContain('type: Idea')
    expect(out).toContain('created: 2026-08-04T10:00:00Z')
    expect(out.endsWith('# Title')).toBe(true)
  })

  it('preserves created and unknown keys when re-saving a loaded idea', () => {
    const s = createStore()
    s.currentFrontmatter = 'type: Idea\ncreated: 2026-01-01T00:00:00Z\nstatus: draft'
    const out = ideaDocText(s, '# Retitled', '2026-08-04T10:00:00Z')
    expect(out).toContain('created: 2026-01-01T00:00:00Z')
    expect(out).toContain('status: draft')
    expect(out).not.toContain('2026-08-04T10:00:00Z')
    expect(out.endsWith('# Retitled')).toBe(true)
  })
})

describe('changeIdeaDir', () => {
  function saved(): SparkStore {
    const s = createStore()
    s.current = '2026-08-04-a.md'
    s.currentFrontmatter = 'type: Idea'
    return s
  }

  it('detaches the open document when the directory actually changes', () => {
    const s = saved()
    expect(changeIdeaDir(s, 'notes/sparks')).toBe(true)
    expect(s.ideaDir).toBe('notes/sparks')
    expect(s.current).toBeNull()
    expect(s.currentFrontmatter).toBeNull()
  })

  it('keeps the open document when the directory only re-normalizes to the same value', () => {
    const s = saved()
    expect(changeIdeaDir(s, ' inbox/ideas/ ')).toBe(true)
    expect(s.current).toBe('2026-08-04-a.md')
    expect(s.currentFrontmatter).toBe('type: Idea')
  })

  it('changes nothing at all when the directory is rejected', () => {
    const s = saved()
    expect(changeIdeaDir(s, '../escape')).toBe(false)
    expect(s.ideaDir).toBe('inbox/ideas')
    expect(s.current).toBe('2026-08-04-a.md')
    expect(s.currentFrontmatter).toBe('type: Idea')
  })
})

// The dirty check must be anchored to what the EDITOR holds, never to the text
// we handed it. moraya's `setContent` dispatches a ProseMirror transaction and
// the lazy change plugin later re-serializes the doc — a round trip that
// normalizes the markdown (a template's trailing newline, for one, does not
// survive it). Baselining on the input would therefore mark an untouched
// document dirty ~200 ms after it loads, and the auto-save-before-switch would
// write files the user never asked for (or re-serialize an agent-written idea).
//
// The stub models exactly that: it stores what a "PM round trip" would produce,
// not what it was given.
function normalizingKit(initial = '') {
  const normalize = (md: string) => md.replace(/\n+$/, '')
  let doc = normalize(initial)
  return {
    setMarkdown: (md: string) => void (doc = normalize(md)),
    getMarkdown: () => doc,
  }
}

describe('showInEditor', () => {
  it('baselines on the editor output, not on the text handed to it', () => {
    const s = createStore()
    const kit = normalizingKit()
    const template = '# New idea\n\n## Outcome\n'

    showInEditor(s, kit, template)

    expect(kit.getMarkdown()).toBe('# New idea\n\n## Outcome')
    expect(s.savedMarkdown).toBe(kit.getMarkdown())
    expect(s.savedMarkdown).not.toBe(template)
    expect(s.dirty).toBe(false)
  })

  it("the editor's own delayed echo cannot fabricate dirt", () => {
    const s = createStore()
    const kit = normalizingKit()

    showInEditor(s, kit, '# New idea\n\n## Outcome\n')
    // ~200 ms later the change plugin reports the serialized doc.
    markEdited(s, kit.getMarkdown())

    expect(s.dirty).toBe(false)
    expect(needsSaveBefore(s, kit.getMarkdown())).toBe(false)
  })

  it('takes the text verbatim when there is no kit (the fallback textarea)', () => {
    const s = createStore()
    showInEditor(s, null, '# Draft\n')
    expect(s.savedMarkdown).toBe('# Draft\n')
    expect(s.dirty).toBe(false)
  })
})

describe('rebaseline', () => {
  it('adopts the editor content as the baseline (used right after mounting)', () => {
    const s = createStore()
    const kit = normalizingKit('# New idea\n\n## Outcome\n')
    s.savedMarkdown = '# New idea\n\n## Outcome\n' // what we asked the kit to mount

    rebaseline(s, kit)

    expect(s.savedMarkdown).toBe('# New idea\n\n## Outcome')
    expect(s.dirty).toBe(false)
    expect(needsSaveBefore(s, kit.getMarkdown())).toBe(false)
  })
})

describe('needsSaveBefore', () => {
  it('asks the live buffer, so an edit inside the debounce window still counts', () => {
    const s = createStore()
    const kit = normalizingKit()
    showInEditor(s, kit, '# Idea')
    // The user types; `dirty` is still false because the 200 ms debounce has
    // not fired — but the live buffer already differs and must not be lost.
    kit.setMarkdown('# Idea\n\nthe part that would be lost')

    expect(s.dirty).toBe(false)
    expect(needsSaveBefore(s, kit.getMarkdown())).toBe(true)
  })

  it('is false when the live buffer matches the baseline', () => {
    const s = createStore()
    s.savedMarkdown = '# Idea'
    expect(needsSaveBefore(s, '# Idea')).toBe(false)
  })
})

describe('markEdited', () => {
  it('raises and lowers dirty against the baseline', () => {
    const s = createStore()
    s.savedMarkdown = '# Idea'
    markEdited(s, '# Idea and more')
    expect(s.dirty).toBe(true)
    markEdited(s, '# Idea')
    expect(s.dirty).toBe(false)
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
  it('starts a new idea blank — no template', () => {
    expect(ideaTemplate()).toBe('')
  })
})

describe('isBlank', () => {
  it('is true for an empty document (what a fresh draft holds)', () => {
    expect(isBlank('')).toBe(true)
  })

  it('is true for whitespace only — a stray newline is not an idea', () => {
    expect(isBlank('\n')).toBe(true)
    expect(isBlank('   \n\t \r\n')).toBe(true)
  })

  it('is false as soon as there is any real content', () => {
    expect(isBlank('a')).toBe(false)
    expect(isBlank('\n\n  x  \n')).toBe(false)
  })
})

describe('clockTime', () => {
  it('is local HH:mm, zero-padded on both fields', () => {
    expect(clockTime(new Date(2026, 7, 4, 9, 5))).toBe('09:05')
    expect(clockTime(new Date(2026, 7, 4, 19, 42))).toBe('19:42')
  })

  it('renders midnight as 00:00, not 24:00', () => {
    expect(clockTime(new Date(2026, 7, 4, 0, 0))).toBe('00:00')
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
