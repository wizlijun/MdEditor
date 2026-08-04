import { describe, it, expect } from 'vitest'
import { isReservedConceptName } from './okf/concept'
import { slugFromMarkdown, ideaFileName, proofPathFor, timestampFileName } from './naming'

describe('slugFromMarkdown', () => {
  it('takes the first heading line, strips the # marker and collapses whitespace', () => {
    expect(slugFromMarkdown('# 我 的 绝妙点子\n\n正文')).toBe('我-的-绝妙点子')
  })

  it('takes a plain first line when the document has no heading at all', () => {
    expect(slugFromMarkdown('这只是一段普通话,不是标题。\n\n# 后面才是标题')).toBe('这只是一段普通话,不是标题。')
  })

  it('falls back to "idea" for an empty document', () => {
    expect(slugFromMarkdown('')).toBe('idea')
    expect(slugFromMarkdown('\n\n   \n')).toBe('idea')
  })

  it('falls back to "idea" when the document only has YAML frontmatter and no body', () => {
    expect(slugFromMarkdown('---\ntype: Idea\ncreated: 2026-08-04\n---\n')).toBe('idea')
  })

  it('skips a leading frontmatter block and takes the real title from the body', () => {
    // The load-bearing case: every saved idea, read back for renaming, has a
    // frontmatter block in front of its title. Regression for a bug where
    // the closing `---` fence line itself was mistaken for the title and
    // collapsed to '' → 'idea'.
    expect(slugFromMarkdown('---\ntype: Idea\n---\n\n# Real Title Here\n\nbody')).toBe('Real-Title-Here')
  })

  it('does not skip a "---" that is a mid-document separator, not a leading frontmatter fence', () => {
    expect(slugFromMarkdown('# Title\n\n---\n\nMore text')).toBe('Title')
  })

  it('falls back to "idea" when the leading frontmatter fence is never closed (does not swallow the whole doc)', () => {
    expect(slugFromMarkdown('---\ntype: Idea\n\n# Title\nbody')).toBe('idea')
  })

  it('falls back to "idea" for a title made entirely of forbidden characters', () => {
    expect(slugFromMarkdown('???###%%%')).toBe('idea')
  })

  it('keeps a pure-emoji title as-is (nothing in it is forbidden)', () => {
    expect(slugFromMarkdown('🎉✨🚀')).toBe('🎉✨🚀')
  })

  it('strips reserved filesystem/markdown characters wherever they occur', () => {
    expect(slugFromMarkdown('a/b\\c:d*e?f"g<h>i|j#k%l`m')).toBe('abcdefghijklm')
  })

  it('truncates to 40 characters without splitting a surrogate-pair emoji in half', () => {
    // 45 flag emoji (each is a surrogate pair, i.e. 2 UTF-16 code units) — a
    // byte/UTF-16-unit truncation at 40 would slice one flag in half and
    // produce an unpaired surrogate (mangled / throws when re-encoded).
    const flags = '🚩'.repeat(45)
    const slug = slugFromMarkdown(flags)
    expect(Array.from(slug).length).toBeLessThanOrEqual(40)
    expect(slug).toBe('🚩'.repeat(Array.from(slug).length))
    // round-trips through JSON without producing lone surrogates
    expect(() => JSON.parse(JSON.stringify(slug))).not.toThrow()
  })

  it('truncates a long ascii title to exactly 40 characters', () => {
    const title = 'a'.repeat(80)
    expect(slugFromMarkdown(title)).toBe('a'.repeat(40))
  })
})

describe('ideaFileName', () => {
  it('builds "<today>-<slug>.md" for a fresh title', () => {
    expect(ideaFileName('# 我的点子', '2026-08-04', new Set())).toBe('2026-08-04-我的点子.md')
  })

  it('falls back to the "idea" slug for a titleless document', () => {
    expect(ideaFileName('', '2026-08-04', new Set())).toBe('2026-08-04-idea.md')
  })

  it('appends -2, -3, ... on collision, in order', () => {
    const taken = new Set(['2026-08-04-idea.md', '2026-08-04-idea-2.md'])
    expect(ideaFileName('', '2026-08-04', taken)).toBe('2026-08-04-idea-3.md')
  })

  it('never returns a name only one collision away from what is already taken', () => {
    const taken = new Set(['2026-08-04-x.md'])
    expect(ideaFileName('x', '2026-08-04', taken)).toBe('2026-08-04-x-2.md')
  })

  it('never produces a reserved concept name, even in pathological inputs', () => {
    // The mandatory "<today>-" prefix makes an exact collision with the
    // 8/6-char reserved names ("index.md"/"log.md") structurally
    // unreachable, but the guard is still exercised end-to-end here across
    // inputs chosen to get as close as the template allows.
    for (const [md, today] of [['index', ''], ['log', ''], ['index', 'log'], ['', '']] as const) {
      const name = ideaFileName(md, today, new Set())
      expect(isReservedConceptName(name)).toBe(false)
    }
  })
})

describe('timestampFileName', () => {
  const at = new Date(2026, 7, 4, 19, 42) // 本地时间 2026-08-04 19:42
  it('names by creation minute, not by title', () => {
    expect(timestampFileName(at, new Set())).toBe('2026-08-04-1942-idea.md')
  })
  it('pads single-digit month/day/hour/minute', () => {
    expect(timestampFileName(new Date(2026, 0, 2, 3, 4), new Set())).toBe('2026-01-02-0304-idea.md')
  })
  it('suffixes on collision inside the same minute', () => {
    const taken = new Set(['2026-08-04-1942-idea.md', '2026-08-04-1942-idea-2.md'])
    expect(timestampFileName(at, taken)).toBe('2026-08-04-1942-idea-3.md')
  })
})

describe('proofPathFor', () => {
  it('replaces the trailing .md with .proof.md', () => {
    expect(proofPathFor('inbox/ideas/a.md')).toBe('inbox/ideas/a.proof.md')
  })

  it('handles a bare filename with no directory', () => {
    expect(proofPathFor('a.md')).toBe('a.proof.md')
  })
})
