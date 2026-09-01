import { describe, expect, it } from 'vitest'
import {
  buildIdeaDocument,
  DEFAULT_IDEA_DIR,
  isIdeaFileName,
  normalizeVaultDir,
  parseIdeaDir,
  parseIdeaSource,
  proofPathFor,
  sortIdeasNewestFirst,
  timestampIdeaFileName,
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

  it('uses Idea Spark local-time names and avoids both idea and proof collisions', () => {
    const at = new Date(2026, 7, 30, 9, 5)
    expect(timestampIdeaFileName(at, new Set())).toBe('2026-08-30-0905-idea.md')
    expect(timestampIdeaFileName(at, new Set([
      '2026-08-30-0905-idea.md',
      '2026-08-30-0905-2-idea.proof.md',
    ]))).toBe('2026-08-30-0905-3-idea.md')
  })

  it('builds an OKF Idea with planning defaults while preserving the body', () => {
    const body = '# 一个念头\n\n---\n\n继续说明'
    const document = buildIdeaDocument(body, '2026-08-30T01:05:00.000Z')
    expect(document).toBe('---\ntype: Idea\ncreated: 2026-08-30T01:05:00.000Z\nnext:\n  priority: P2\n---\n' + body)
  })

  it('round-trips namespaced priority, quoted due date, and contexts', () => {
    const document = buildIdeaDocument('# 行动', '2026-09-01T01:00:00Z', {
      priority: 'P0', due: '2026-09-08', contexts: ['@电脑', '@电话'],
    })
    expect(document).toContain('due: "2026-09-08"')
    expect(parseIdeaSource('inbox/ideas/a-idea.md', document, false)).toMatchObject({
      priority: 'P0', due: '2026-09-08', contexts: ['@电脑', '@电话'],
    })
  })

  it('keeps legacy and malformed Next metadata readable', () => {
    expect(parseIdeaSource('inbox/ideas/a-idea.md', '---\ntype: Idea\nnext:\n  priority: impossible\n  due: tomorrow\n  contexts: nope\n---\n# Legacy', false)).toMatchObject({
      title: 'Legacy', body: '# Legacy', proofed: false,
    })
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
      body: '# Build it',
      proofed: true,
    })
  })

  it('preserves every human-authored body line for previews, with or without frontmatter', () => {
    const body = '# 标题\n\n第一段。\n\n- 细节一\n- 细节二\n\n最后一段。'
    expect(parseIdeaSource(
      'inbox/ideas/a-idea.md',
      `---\ntype: Idea\ncreated: 2026-08-29T01:02:03Z\n---\n${body}`,
      false,
    ).body).toBe(body)
    expect(parseIdeaSource('inbox/ideas/b-idea.md', body, false).body).toBe(body)
  })

  it('sorts reliable creation times first and newest', () => {
    const items = [
      { path: 'z-idea.md', title: 'z', body: 'z', proofed: false },
      { path: 'a-idea.md', title: 'a', body: 'a', proofed: false, created: '2026-08-28T00:00:00Z' },
      { path: 'b-idea.md', title: 'b', body: 'b', proofed: false, created: '2026-08-29T00:00:00Z' },
    ]
    expect(sortIdeasNewestFirst(items).map((item) => item.title)).toEqual(['b', 'a', 'z'])
  })
})
