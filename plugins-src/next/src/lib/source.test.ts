import { describe, expect, it } from 'vitest'
import {
  DEFAULT_IDEA_DIR,
  isIdeaFileName,
  normalizeVaultDir,
  parseIdeaDir,
  parseIdeaSource,
  proofPathFor,
  sortIdeasNewestFirst,
  titleFromMarkdown,
} from './source'

describe('idea source contract', () => {
  it('accepts the historic *-idea.md names only', () => {
    expect(isIdeaFileName('2026-08-29-1341-idea.md')).toBe(true)
    expect(isIdeaFileName('thought.idea.md')).toBe(false)
    expect(isIdeaFileName('2026-08-29-1341-idea.proof.md')).toBe(false)
  })

  it('derives the proof sidecar without changing the idea path', () => {
    expect(proofPathFor('inbox/ideas/a-idea.md')).toBe('inbox/ideas/a-idea.proof.md')
  })

  it('rejects absolute and traversal directories', () => {
    expect(normalizeVaultDir('/tmp/ideas')).toBeNull()
    expect(normalizeVaultDir('../ideas')).toBeNull()
    expect(normalizeVaultDir('inbox/../ideas')).toBeNull()
    expect(normalizeVaultDir('inbox\\ideas/')).toBe('inbox/ideas')
  })

  it('reads Idea Spark configuration without trusting malformed input', () => {
    expect(parseIdeaDir('{"ideaDir":"sparks"}')).toBe('sparks')
    expect(parseIdeaDir('{broken')).toBe(DEFAULT_IDEA_DIR)
    expect(parseIdeaDir('{"ideaDir":"../../outside"}')).toBe(DEFAULT_IDEA_DIR)
  })

  it('uses frontmatter title, then the first readable body line', () => {
    expect(titleFromMarkdown('---\ntitle: A title\n---\n# Body', 'fallback')).toBe('A title')
    expect(titleFromMarkdown('---\ncreated: now\n---\n\n# Body title', 'fallback')).toBe('Body title')
    expect(titleFromMarkdown('', '2026-idea.md')).toBe('2026')
  })

  it('reads created while leaving proof as independent evidence', () => {
    const source = parseIdeaSource(
      'inbox/ideas/a-idea.md',
      '---\ntype: Idea\ncreated: 2026-08-29T01:02:03Z\n---\n# Build it',
      true,
    )
    expect(source).toEqual({
      path: 'inbox/ideas/a-idea.md',
      created: '2026-08-29T01:02:03Z',
      title: 'Build it',
      proofed: true,
    })
  })

  it('sorts reliable creation times first and newest', () => {
    const items = [
      { path: 'z-idea.md', title: 'z', proofed: false },
      { path: 'a-idea.md', title: 'a', proofed: false, created: '2026-08-28T00:00:00Z' },
      { path: 'b-idea.md', title: 'b', proofed: false, created: '2026-08-29T00:00:00Z' },
    ]
    expect(sortIdeasNewestFirst(items).map((item) => item.title)).toEqual(['b', 'a', 'z'])
  })
})
