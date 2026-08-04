// Tests for the pure state transitions in store.svelte.ts.
//
// Only the *pure* half is covered here: transitions take an explicit store
// object (created by `createStore()`), never the module-level `$state`
// singleton, so a test can build an isolated store, apply a transition and
// assert on the result without a Svelte component tree or a host bridge.
// The async actions (boot/reload/save/...) are bridge IO and are exercised
// by hand in the window — see the task report's manual-verification list.
import { beforeEach, describe, it, expect, vi } from 'vitest'

// The action half (`deleteIdea` / `renameIdea` / …) is bridge IO, and the parts
// of it worth pinning are its ERROR paths — what the store does when the second
// of two removals fails, or what `state.current` says while a rename is only
// half applied. Those can't be reached by hand in a window often enough to
// trust, so the bridge is stubbed and the actions are driven directly. Declared
// with `vi.hoisted` because `vi.mock`'s factory is hoisted above the imports.
const host = vi.hoisted(() => ({
  request: vi.fn(),
  agentRun: vi.fn(),
  agentStatus: vi.fn(),
  vaultInfo: vi.fn(),
  vaultRead: vi.fn(),
  vaultWrite: vi.fn(),
  vaultExists: vi.fn(),
  vaultList: vi.fn(),
  vaultRemove: vi.fn(),
  vaultRename: vi.fn(),
}))
vi.mock('./bridge', () => ({
  bridge: () => ({ pluginId: 'test', locale: 'en', theme: 'x', request: host.request, onMessage: () => {} }),
  agentRun: host.agentRun,
  agentStatus: host.agentStatus,
  vaultInfo: host.vaultInfo,
  vaultRead: host.vaultRead,
  vaultWrite: host.vaultWrite,
  vaultExists: host.vaultExists,
  vaultList: host.vaultList,
  vaultRemove: host.vaultRemove,
  vaultRename: host.vaultRename,
}))

import {
  applyRunDone,
  bodyOf,
  changeIdeaDir,
  clockTime,
  createdFromName,
  createStore,
  deleteIdea,
  filesToDelete,
  loadIdea,
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
  reconcilePending,
  runInFlight,
  relativeAge,
  relPath,
  renameIdea,
  runStatusWord,
  rowTitle,
  showInEditor,
  setIdeaDir,
  state,
  statusOf,
  titleOf,
  validateRename,
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

// ── inbox: deletion, renaming, row labels ───────────────────────────────────

describe('filesToDelete', () => {
  it('includes the proof sidecar when it exists', () => {
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    s.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md', 'inbox/ideas/b.md']
    expect(filesToDelete(s, 'a.md')).toEqual(['inbox/ideas/a.md', 'inbox/ideas/a.proof.md'])
    expect(filesToDelete(s, 'b.md')).toEqual(['inbox/ideas/b.md'])
  })

  it('lists the idea even when the listing never saw it (a stale panel row)', () => {
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    s.files = []
    expect(filesToDelete(s, 'a.md')).toEqual(['inbox/ideas/a.md'])
  })
})

describe('validateRename', () => {
  const s = createStore()
  s.ideaDir = 'inbox/ideas'
  s.files = ['inbox/ideas/a.md', 'inbox/ideas/taken.md']

  it('appends .md and accepts a free name', () => {
    expect(validateRename(s, 'a.md', '新名字')).toEqual({ ok: true, name: '新名字.md' })
    expect(validateRename(s, 'a.md', '新名字.md')).toEqual({ ok: true, name: '新名字.md' })
  })

  it('renaming to its own name is fine', () => {
    expect(validateRename(s, 'a.md', 'a')).toEqual({ ok: true, name: 'a.md' })
  })

  it('trims the surrounding whitespace before judging the name', () => {
    expect(validateRename(s, 'a.md', '  spaced  ')).toEqual({ ok: true, name: 'spaced.md' })
  })

  it('rejects empty, slashes, leading dots and taken names', () => {
    expect(validateRename(s, 'a.md', '   ')).toEqual({ ok: false, reason: 'empty' })
    expect(validateRename(s, 'a.md', 'x/y')).toEqual({ ok: false, reason: 'slash' })
    expect(validateRename(s, 'a.md', '.hidden')).toEqual({ ok: false, reason: 'dot' })
    expect(validateRename(s, 'a.md', 'taken')).toEqual({ ok: false, reason: 'taken' })
  })

  it("treats an existing sidecar's own name as taken too", () => {
    const withProof = createStore()
    withProof.ideaDir = 'inbox/ideas'
    withProof.files = ['inbox/ideas/a.md', 'inbox/ideas/b.proof.md']
    expect(validateRename(withProof, 'a.md', 'b.proof')).toEqual({ ok: false, reason: 'taken' })
  })

  // `listIdeas` drops `*.proof.md` from the listing (a sidecar describes an
  // idea, it isn't one), so an idea allowed to take that suffix would vanish
  // from the inbox the moment it was renamed: still on disk, no error, no row
  // left to undo it from — and, if it was the open document, autosave still
  // writing into a file with no row. The suffix is refused on its own terms,
  // NOT merely because some file happens to sit at that name.
  it.each(['c.proof', 'c.proof.md', '方案A.proof'])('refuses the sidecar suffix %o', (raw) => {
    expect(validateRename(s, 'a.md', raw)).toEqual({ ok: false, reason: 'taken' })
  })

  // The mirror image: `b.md` is free but an orphaned `b.proof.md` is lying
  // around. Renaming into it would make the idea claim a `done` badge and an
  // "open the argument" item pointing at a document that argues something else.
  it("refuses a name whose sidecar slot is already occupied", () => {
    const orphan = createStore()
    orphan.ideaDir = 'inbox/ideas'
    orphan.files = ['inbox/ideas/a.md', 'inbox/ideas/b.proof.md']
    expect(orphan.files).not.toContain('inbox/ideas/b.md') // the name itself is free
    expect(validateRename(orphan, 'a.md', 'b')).toEqual({ ok: false, reason: 'taken' })
  })

  // `index.md` / `log.md` are OKF-reserved structural documents (see
  // okf/concept.ts). Letting an idea take one of those names would both break
  // the format contract and make the row vanish from the inbox, since
  // `listIdeas` filters reserved names out — so the name is refused as
  // unavailable, which is exactly what `taken` means to the user.
  it.each(['index', 'log', 'index.md'])('refuses the OKF-reserved name %o', (raw) => {
    expect(validateRename(s, 'a.md', raw)).toEqual({ ok: false, reason: 'taken' })
  })
})

describe('rowTitle', () => {
  it('reads the H1 out of the body AS WRITTEN — spaces and all', () => {
    // Not `Ship-the-thing`: the row shows the document's title, not the file
    // name that could be derived from it (`slugFromMarkdown`'s job).
    expect(rowTitle('# Ship the thing\n\nbody', '2026-08-04-1942-idea.md')).toBe('Ship the thing')
  })

  it('does not truncate — the 240px column ellipsizes in CSS, which says so', () => {
    const long = `${'长'.repeat(60)}`
    expect(rowTitle(`# ${long}`, 'x.md')).toBe(long)
  })

  it('falls back to the file name when the body yields no title', () => {
    expect(rowTitle('', '2026-08-04-1942-idea.md')).toBe('1942-idea')
    expect(rowTitle('   \n\n', '2026-08-04-1942-idea.md')).toBe('1942-idea')
  })

  it('skips frontmatter rather than titling the row `type: Idea`', () => {
    expect(rowTitle('---\ntype: Idea\n---\n\n# Real title', 'x.md')).toBe('Real title')
  })
})

describe('titleOf', () => {
  it('uses the cached title once the body has been read', () => {
    const s = createStore()
    expect(titleOf(s, '2026-08-04-1942-idea.md')).toBe('1942-idea')
    s.titles = { '2026-08-04-1942-idea.md': 'Ship-the-thing' }
    expect(titleOf(s, '2026-08-04-1942-idea.md')).toBe('Ship-the-thing')
  })
})

describe('createdFromName', () => {
  it('reads the creation minute out of a timestamp name', () => {
    expect(createdFromName('2026-08-04-1942-idea.md')).toEqual(new Date(2026, 7, 4, 19, 42))
  })

  it('reads a date-only name as local midnight', () => {
    expect(createdFromName('2026-08-04-my-idea.md')).toEqual(new Date(2026, 7, 4, 0, 0))
  })

  it('is null for a name that carries no date (a renamed idea)', () => {
    expect(createdFromName('my-idea.md')).toBeNull()
  })

  it('is null for an impossible date rather than rolling it over', () => {
    expect(createdFromName('2026-13-45-idea.md')).toBeNull()
    expect(createdFromName('2026-08-04-2599-idea.md')).toBeNull()
  })
})

describe('relativeAge', () => {
  const now = new Date(2026, 7, 4, 12, 0)
  it.each([
    [new Date(2026, 7, 4, 11, 58), -2, 'minute'],
    [new Date(2026, 7, 4, 9, 0), -3, 'hour'],
    [new Date(2026, 7, 1, 12, 0), -3, 'day'],
    [new Date(2026, 3, 4, 12, 0), -4, 'month'],
    [new Date(2022, 7, 4, 12, 0), -4, 'year'],
  ])('%o → %i %s ago', (from, value, unit) => {
    expect(relativeAge(from as Date, now)).toEqual({ value, unit })
  })

  it('rounds a fresh idea down to "0 minutes", never into the future', () => {
    expect(relativeAge(new Date(2026, 7, 4, 12, 0, 30), now)).toEqual({ value: 0, unit: 'minute' })
    // Clock skew (a file stamped ahead of us) must not read as "in 5 minutes".
    expect(relativeAge(new Date(2026, 7, 4, 12, 5), now)).toEqual({ value: 0, unit: 'minute' })
  })
})

// ── actions, driven against a stubbed bridge ────────────────────────────────
//
// Only the paths that a hand test in the window would never reliably reproduce:
// a removal that half succeeds, and the window between a rename's two host
// calls. Both are places where getting it wrong destroys or duplicates a user's
// document, and neither shows up in the pure transitions above.

describe('deleteIdea', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.assign(state, createStore())
    state.booting = false
    state.vaultRoot = '/vault'
    state.ideaDir = 'inbox/ideas'
    // Everything the actions call incidentally: the re-list, the state-file
    // write behind `newIdea()`, and the toast.
    host.vaultList.mockResolvedValue({ entries: [] })
    host.vaultWrite.mockResolvedValue({ ok: true })
    host.request.mockResolvedValue({})
  })

  /** One saved idea, open in the editor, with a proof sidecar next to it. */
  function openIdeaWithProof(): void {
    state.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md']
    state.docs = ['a.md']
    state.current = 'a.md'
    state.currentFrontmatter = 'type: Idea'
    state.savedMarkdown = '# Ship it'
    state.titles = { 'a.md': 'Ship it' }
  }

  it('removes the idea first, then its sidecar', async () => {
    openIdeaWithProof()
    host.vaultRemove.mockResolvedValue({ ok: true })

    await deleteIdea('a.md')

    expect(host.vaultRemove.mock.calls.map((c) => c[0])).toEqual([
      'inbox/ideas/a.md',
      'inbox/ideas/a.proof.md',
    ])
  })

  // The regression that matters: the idea is deleted first, so "the SIDECAR's
  // removal failed" still means the idea itself is gone for good. Reporting
  // that as a plain failure would leave `current` pointing at the deleted file
  // with its text still in the editor — and the next keystroke's autosave
  // (which asks `freeFileName`, which hands back `state.current` unchanged)
  // would write the document the user was told had been deleted back to disk.
  it('detaches the open document when the idea is gone but the sidecar failed', async () => {
    openIdeaWithProof()
    host.vaultRemove
      .mockResolvedValueOnce({ ok: true })
      .mockRejectedValueOnce(new Error('io: sidecar is busy'))

    const blank = await deleteIdea('a.md')

    expect(host.vaultRemove).toHaveBeenCalledTimes(2)
    // Non-null = "show this in the editor": the deleted idea must not stay on
    // screen attached to a file that no longer exists.
    expect(blank).toBe('')
    expect(state.current).toBeNull()
    expect(state.currentFrontmatter).toBeNull()
    expect(state.savedMarkdown).toBe('')
    expect(state.titles['a.md']).toBeUndefined()
  })

  it('keeps the document attached when the IDEA itself could not be removed', async () => {
    openIdeaWithProof()
    host.vaultRemove.mockRejectedValue(new Error('io: permission denied'))

    const blank = await deleteIdea('a.md')

    expect(blank).toBeNull()
    expect(state.current).toBe('a.md')
    expect(state.currentFrontmatter).toBe('type: Idea')
    // Nothing was deleted, so the row's cached label must survive too.
    expect(state.titles['a.md']).toBe('Ship it')
    // The sidecar is never attempted once the idea's own removal failed.
    expect(host.vaultRemove).toHaveBeenCalledTimes(1)
  })

  // `runInFlight` is a GLOBAL gate, so a `pending` entry left behind by a
  // deleted idea doesn't just mis-badge one row — it disables delegation for
  // the whole plugin, is written to `.notemd/idea-spark.json` by `persist()`
  // (so it outlives the window), and `reconcilePending` won't clear it when the
  // agent is unreachable. The only recovery would be editing the JSON by hand.
  it('drops the deleted idea\'s pending run so delegation is not wedged', async () => {
    openIdeaWithProof()
    state.pending = { 'inbox/ideas/a.md': 'run-1' }
    state.failed = ['inbox/ideas/a.md']
    expect(runInFlight(state)).toBe(true)
    host.vaultRemove.mockResolvedValue({ ok: true })

    await deleteIdea('a.md')

    expect(state.pending).toEqual({})
    expect(state.failed).toEqual([])
    expect(runInFlight(state)).toBe(false)
  })

  // The other half: a run belonging to an idea that is still there must survive
  // its neighbour's deletion, or deleting any row would silently orphan it.
  it('keeps another idea\'s pending run', async () => {
    openIdeaWithProof()
    state.files = [...state.files, 'inbox/ideas/b.md']
    state.docs = ['b.md', 'a.md']
    state.pending = { 'inbox/ideas/a.md': 'run-1' }
    host.vaultRemove.mockResolvedValue({ ok: true })

    await deleteIdea('b.md')

    expect(state.pending).toEqual({ 'inbox/ideas/a.md': 'run-1' })
  })

  it('leaves another idea open when a different row is deleted', async () => {
    openIdeaWithProof()
    state.files = [...state.files, 'inbox/ideas/b.md']
    state.docs = ['b.md', 'a.md']
    host.vaultRemove.mockResolvedValue({ ok: true })

    const blank = await deleteIdea('b.md')

    expect(blank).toBeNull()
    expect(state.current).toBe('a.md')
  })
})

describe('renameIdea', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.assign(state, createStore())
    state.booting = false
    state.vaultRoot = '/vault'
    state.ideaDir = 'inbox/ideas'
    state.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md']
    state.docs = ['a.md']
    state.current = 'a.md'
    state.titles = { 'a.md': 'Ship it' }
    host.vaultList.mockResolvedValue({ entries: [] })
    host.vaultWrite.mockResolvedValue({ ok: true })
    host.request.mockResolvedValue({})
    host.vaultRename.mockResolvedValue({ ok: true })
  })

  // `state.current` is what autosave writes to, and the two renames are two
  // full IPC round trips apart. A change echo landing in that window (the kit
  // reports edits ~200 ms late) would schedule a write to the OLD path and
  // recreate the file the rename just moved — two copies of one idea. So every
  // in-memory key has to move the instant the idea's own rename lands, before
  // the sidecar's call is even started.
  it('re-points current before the sidecar round trip, not after', async () => {
    let currentDuringSidecar: string | null = 'not called'
    host.vaultRename.mockImplementation(async (from: string) => {
      if (from.endsWith('.proof.md')) currentDuringSidecar = state.current
      return { ok: true }
    })

    await renameIdea('a.md', 'b')

    expect(currentDuringSidecar).toBe('b.md')
    expect(state.current).toBe('b.md')
  })

  it('carries the pending run, the failure record and the cached title across', async () => {
    state.pending = { 'inbox/ideas/a.md': 'run-1' }
    state.failed = ['inbox/ideas/a.md']

    await renameIdea('a.md', 'b')

    expect(state.pending).toEqual({ 'inbox/ideas/b.md': 'run-1' })
    expect(state.failed).toEqual(['inbox/ideas/b.md'])
    expect(state.titles).toEqual({ 'b.md': 'Ship it' })
  })

  it('moves the sidecar with the idea', async () => {
    await renameIdea('a.md', 'b')
    expect(host.vaultRename.mock.calls).toEqual([
      ['inbox/ideas/a.md', 'inbox/ideas/b.md'],
      ['inbox/ideas/a.proof.md', 'inbox/ideas/b.proof.md'],
    ])
  })

  it('changes nothing on disk when the name is refused', async () => {
    // `.proof` would make the row vanish from the inbox; the guard belongs in
    // front of the host call, not after it.
    expect(await renameIdea('a.md', 'b.proof')).toBe(false)
    expect(await renameIdea('a.md', '  ')).toBe(false)
    expect(await renameIdea('a.md', 'x/y')).toBe(false)
    expect(host.vaultRename).not.toHaveBeenCalled()
    expect(state.current).toBe('a.md')
  })

  it('leaves everything attached to the old name when the host refuses', async () => {
    host.vaultRename.mockRejectedValue(new Error('io: destination already exists'))

    expect(await renameIdea('a.md', 'b')).toBe(false)

    expect(state.current).toBe('a.md')
    expect(state.titles).toEqual({ 'a.md': 'Ship it' })
    // The sidecar is not moved on its own when the idea did not move.
    expect(host.vaultRename).toHaveBeenCalledTimes(1)
  })
})

describe('loadIdea', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.assign(state, createStore())
    state.booting = false
    state.vaultRoot = '/vault'
    state.ideaDir = 'inbox/ideas'
    state.docs = ['a.md']
    state.files = ['inbox/ideas/a.md']
    host.request.mockResolvedValue({})
  })

  // The cached row label is otherwise only maintained by this window's own
  // writes — but "open in the main editor" sends the user off to edit the file
  // somewhere else, and an agent or a vault sync can rewrite one at any time.
  // Opening an idea has the whole document in hand, so correcting the label
  // costs no extra IO and is the cheapest of the cache's honesty checks.
  it('refreshes the cached row title from the document it just read', async () => {
    state.titles = { 'a.md': 'the old heading' }
    host.vaultRead.mockResolvedValue({ content: '---\ntype: Idea\n---\n\n# A better heading\n\nbody' })

    const body = await loadIdea('a.md')

    expect(body).toBe('# A better heading\n\nbody')
    expect(state.titles['a.md']).toBe('A better heading')
  })

  it('leaves the cache alone when the read fails', async () => {
    state.titles = { 'a.md': 'the old heading' }
    host.vaultRead.mockRejectedValue(new Error('io: gone'))

    expect(await loadIdea('a.md')).toBeNull()
    expect(state.titles['a.md']).toBe('the old heading')
  })
})

// The gate on delegation. claude-agent locks a task's run directory for the
// duration of a run ("Same task mutually exclusive", lock.rs), and every idea
// this plugin delegates uses the SAME task — so the rule is one run at a time,
// full stop. Getting this wrong doesn't fail loudly: `run-task` still hands
// back a run id, the refusal happens inside the spawned task, no record is
// ever written, and the second idea surfaces two seconds later as `lost` — a
// ⚠ and a "the agent couldn't argue this" about an idea nothing ever tried.
describe('runInFlight', () => {
  it('is false with nothing pending and true from the first run onwards', () => {
    const s = createStore()
    expect(runInFlight(s)).toBe(false)
    s.pending = { 'inbox/ideas/a.md': 'r1' }
    expect(runInFlight(s)).toBe(true)
  })

  it('is true for a run on a DIFFERENT idea than the one being asked about', () => {
    // The whole point: delegating B while A is running is what the per-idea
    // guard used to allow.
    const s = createStore()
    s.pending = { 'inbox/ideas/a.md': 'r1' }
    expect(runInFlight(s)).toBe(true)
  })

  it('is true for a running idea that already has a proof document', () => {
    // `deriveStatus` ranks `done` above `running`, so a menu keyed on
    // `statusOf` would call this idea `done` — and enable an action the
    // action bar has disabled.
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    s.files = ['inbox/ideas/a.md', 'inbox/ideas/a.proof.md']
    s.pending = { 'inbox/ideas/a.md': 'r2' }
    expect(statusOf(s, 'a.md')).toBe('done')
    expect(runInFlight(s)).toBe(true)
  })

  it('is false again once the run is applied', () => {
    const s = createStore()
    s.ideaDir = 'inbox/ideas'
    s.files = ['inbox/ideas/a.md']
    s.pending = { 'inbox/ideas/a.md': 'r1' }
    applyRunDone(s, { run_id: 'r1', status: 'success' })
    expect(runInFlight(s)).toBe(false)
  })
})

describe('runStatusWord', () => {
  // `applyRunDone` speaks claude-agent's status vocabulary and treats
  // everything that isn't `success` as a failure — so this mapping is the one
  // place a timed-out or cancelled run could be mistaken for a finished one.
  it('is success only for a successful run', () => {
    expect(runStatusWord({ kind: 'done', success: true })).toBe('success')
    expect(runStatusWord({ kind: 'done', success: false })).toBe('error')
    expect(runStatusWord({ kind: 'lost' })).toBe('lost')
  })
})

// Startup reconciliation: `pending` came off disk and was written by an
// EARLIER window, so every entry in it is a claim about a run this process has
// never seen. Getting this wrong is what turns a ⏳ into a permanent one — or,
// worse, marks a run that is still going as failed.
describe('reconcilePending', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.assign(state, createStore())
    state.booting = false
    state.vaultRoot = '/vault'
    state.ideaDir = 'inbox/ideas'
    state.files = ['inbox/ideas/a.md']
    state.docs = ['a.md']
    state.pending = { 'inbox/ideas/a.md': 'r1' }
    host.vaultList.mockResolvedValue({ entries: [{ name: 'a.md', is_dir: false }] })
    host.vaultWrite.mockResolvedValue({ ok: true })
    host.request.mockResolvedValue({})
  })

  it('asks about the run under the plugin OWN task, never the agent default', async () => {
    host.agentStatus.mockResolvedValue({ state: 'lost' })
    await reconcilePending()
    // `host.agent.status` defaults a missing task to `answer-note-question`,
    // which would report on another plugin's run directory entirely.
    expect(host.agentStatus).toHaveBeenCalledWith('idea-proof', 'r1')
  })

  it('folds a run that finished while the window was closed into the listing', async () => {
    host.agentStatus.mockResolvedValue({ state: 'done', record: { status: 'success', result: 'ok' } })
    // The re-list is the authority on what is on disk, and by now the agent's
    // proof document is: `applyRunDone`'s optimistic fold only bridges the gap
    // until this listing lands.
    host.vaultList.mockResolvedValue({
      entries: [
        { name: 'a.md', is_dir: false },
        { name: 'a.proof.md', is_dir: false },
      ],
    })

    const still = await reconcilePending()

    expect(still).toEqual([])
    expect(state.pending).toEqual({})
    expect(statusOf(state, 'a.md')).toBe('done')
    // Dropped from disk too, or the next window would ask about it again.
    const written = host.vaultWrite.mock.calls.find(([p]) => p === '.notemd/idea-spark.json')
    expect(JSON.parse(written![1]).pendingRuns).toEqual({})
  })

  it('does not throw confetti for a run the user was not there to watch', async () => {
    host.agentStatus.mockResolvedValue({ state: 'done', record: { status: 'success', result: 'ok' } })
    await reconcilePending()
    // claude-agent already told them, in the tray, while they were away.
    expect(state.celebrate).toBe(false)
  })

  it('marks a run whose process died as failed', async () => {
    host.agentStatus.mockResolvedValue({ state: 'lost' })

    expect(await reconcilePending()).toEqual([])
    expect(state.pending).toEqual({})
    expect(state.failed).toEqual(['inbox/ideas/a.md'])
  })

  it('hands back a run that is still going so the window can watch it again', async () => {
    host.agentStatus.mockResolvedValue({ state: 'running', steps: 4, last: 'Read a.md' })

    expect(await reconcilePending()).toEqual([{ ideaRel: 'inbox/ideas/a.md', runId: 'r1' }])
    expect(state.pending).toEqual({ 'inbox/ideas/a.md': 'r1' })
    expect(statusOf(state, 'a.md')).toBe('running')
  })

  it('leaves the entry alone when the agent cannot be reached at all', async () => {
    // Uninstalled/disabled since the run started. That is not evidence the run
    // failed, and saying it did would be a lie that outlives the outage.
    host.agentStatus.mockRejectedValue(new Error('-32000: agent_unavailable: unknown v2 plugin'))

    const still = await reconcilePending()

    expect(still).toEqual([])
    expect(state.pending).toEqual({ 'inbox/ideas/a.md': 'r1' })
    expect(state.failed).toEqual([])
  })
})
