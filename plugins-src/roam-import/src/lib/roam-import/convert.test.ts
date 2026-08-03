import { describe, it, expect } from 'vitest'
import { convertPage } from './convert'
// 宿主的真解析器:导入产物必须被它读回成同一棵树(plugin 侧只带了 serializeOutline)
import { parseOutline } from '../../../../../src/lib/outline/markdown'
// @ts-expect-error - 宿主仓库的纯 JS lint core(OKF 硬约束校验)
import { lintText } from '../../../../../scripts/okf-lint-core.mjs'

const page = {
  title: '某个概念',
  uid: 'abc123',
  'create-time': 1_600_000_000_000,
  'edit-time': 1_700_000_000_000,
  children: [{ uid: 'b1', string: '一条内容' }],
}

describe('convertPage — OKF frontmatter', () => {
  it('stamps the concept type so the imported page satisfies the hard constraints', () => {
    const out = convertPage(page, new Map())
    expect(out.text).toContain('type: Outline Note')
    expect(lintText('某个概念.note.md', out.text)).toEqual([])
  })

  it('keeps the daily page type distinct', () => {
    const daily = { ...page, title: 'August 15th, 2022', uid: '08-15-2022' }
    const out = convertPage(daily, new Map())
    expect(out.text).toContain('type: Daily Note')
    expect(lintText('2022-08-15.note.md', out.text)).toEqual([])
  })
})

/** The bug this file's `persistId` rule exists for: a page written by this
 *  importer and later synced by the Rust CLI (`backend/src/convert.rs`, which
 *  writes `id::` on EVERY block) has to align block-for-block. An id-less
 *  block matches nothing in that merge, so it survives as a "local block"
 *  beside the Roam copy and the page doubles on every sync. */
describe('convertPage — every block stays addressable', () => {
  const nested = {
    title: 'Nesting',
    uid: 'pg1',
    children: [
      { uid: 'top1', string: 'top level', children: [{ uid: 'kid1', string: 'child', children: [{ uid: 'kid2', string: 'grandchild' }] }] },
      { uid: 'top2', string: 'referenced by nobody' },
    ],
  }

  it('writes id:: for every block, referenced or not, at every depth', () => {
    const out = convertPage(nested, new Map())
    for (const uid of ['top1', 'kid1', 'kid2', 'top2']) {
      expect(out.text).toContain(`id:: ${uid}`)
    }
    const tree = parseOutline(out.text)
    expect([...tree.nodes.values()].every((n) => n.persistId === true)).toBe(true)
    expect([...tree.nodes.keys()].sort()).toEqual(['kid1', 'kid2', 'top1', 'top2'])
  })

  it('leaves a block Roam exported without a uid unpersisted', () => {
    // Its id is a fresh UUID, different on every run — writing it would only
    // create a new phantom identity each import.
    const out = convertPage({ title: 'No uid', uid: 'pg2', children: [{ string: 'orphan' }] }, new Map())
    expect(out.text).not.toContain('id:: ')
  })
})

/** Structural shapes a Roam block's text can forge. Each one, unescaped, costs
 *  the block the `id::` that is its identity — after which the merge re-creates
 *  it on every sync and the note multiplies without bound. Asserted through the
 *  host's real `parseOutline`, because that is the parser they must survive. */
describe('convertPage — blocks that look like outline structure', () => {
  const pageOf = (children: Array<Record<string, unknown>>) => ({ title: 'Shapes', uid: 'pg3', children })
  const nodesOf = (text: string) => [...parseOutline(text).nodes.values()]

  it('keeps a property-shaped continuation line as content', () => {
    const out = convertPage(pageOf([{ uid: 'p1', string: 'meeting notes\nstatus:: open\nanswered:: yes\nby:: me' }]), new Map())
    const n = nodesOf(out.text).find((x) => x.id === 'p1')
    expect(n?.content).toBe('meeting notes\n status:: open\n answered:: yes\n by:: me')
    expect(n?.status).toBeUndefined()
    expect(n?.answeredBy).toBeUndefined()
  })

  it('keeps a shift-enter list as content rather than child bullets', () => {
    const out = convertPage(pageOf([{ uid: 'b1', string: 'shopping\n- milk\n- eggs' }]), new Map())
    const nodes = nodesOf(out.text)
    expect(nodes.find((n) => n.id === 'b1')?.content).toBe('shopping\n - milk\n - eggs')
    expect(nodes.some((n) => n.content === 'milk')).toBe(false)
    expect(nodes.filter((n) => n.parentId === 'b1')).toHaveLength(0)
    expect(nodes).toHaveLength(1)
  })

  it('leaves a fenced list inside a block byte-identical', () => {
    const code = '```yaml\nsteps:\n- foo\n- bar\n```'
    const out = convertPage(pageOf([{ uid: 'c1', string: code }]), new Map())
    expect(out.text).toContain('  - foo')
    expect(nodesOf(out.text).find((n) => n.id === 'c1')?.content).toBe(code)
  })

  it('closes a fence the Roam block never closed so the next block survives', () => {
    const out = convertPage(pageOf([{ uid: 'f1', string: '```js\nconst x = 1' }, { uid: 'f2', string: 'after' }]), new Map())
    const nodes = nodesOf(out.text)
    expect(nodes.find((n) => n.id === 'f1')?.content).toBe('```js\nconst x = 1\n```')
    expect(nodes.find((n) => n.id === 'f2')?.content).toBe('after')
    expect(nodes.find((n) => n.id === 'f2')?.persistId).toBe(true)
  })

  it('re-serializes to itself, so the shapes survive a second read', () => {
    const out = convertPage(
      pageOf([
        { uid: 'r1', string: 'shopping\n- milk\nid:: not-a-property' },
        { uid: 'r2', string: '```js\nconst x = 1' },
      ]),
      new Map(),
    )
    const tree = parseOutline(out.text)
    expect(tree.nodes.size).toBe(2)
    expect(tree.nodes.get('r1')?.content).toBe('shopping\n - milk\n id:: not-a-property')
  })
})
