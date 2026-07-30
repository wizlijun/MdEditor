import { describe, it, expect } from 'vitest'
import { parseOutline, serializeOutline } from './markdown'
import { collapseAnsweredOnLoad } from './auto-collapse'

const NOTE = `- ?为什么命中率能到 90%?
  type:: question
  status:: answered
  line:: 12
  - \`\`\`answer
    ✦ 因为前缀高度重复。
    \`\`\`
    type:: answer
    by:: claude-code
- ?还没人回答的问题?
  type:: question
  status:: open
  line:: 20
- 一条普通笔记
`

describe('collapseAnsweredOnLoad', () => {
  it('collapses a question that already has an answer', () => {
    const tree = parseOutline(NOTE)
    const auto = collapseAnsweredOnLoad(tree)
    const answered = [...tree.nodes.values()].find((n) => n.status === 'answered')!
    expect(answered.collapsed).toBe(true)
    expect(auto.has(answered.id)).toBe(true)
  })

  it('leaves an open question expanded', () => {
    const tree = parseOutline(NOTE)
    collapseAnsweredOnLoad(tree)
    const open = [...tree.nodes.values()].find((n) => n.status === 'open')!
    expect(open.collapsed).toBe(false)
  })

  it('leaves plain notes alone', () => {
    const tree = parseOutline(NOTE)
    const auto = collapseAnsweredOnLoad(tree)
    const plain = [...tree.nodes.values()].find((n) => n.content === '一条普通笔记')!
    expect(plain.collapsed).toBe(false)
    expect(auto.has(plain.id)).toBe(false)
  })

  it('skips an answered question that has no answer node yet', () => {
    const tree = parseOutline(`- ?问题?\n  type:: question\n  status:: answered\n`)
    expect(collapseAnsweredOnLoad(tree).size).toBe(0)
  })

  it('does not claim a collapse the file already recorded', () => {
    const tree = parseOutline(NOTE.replace('  status:: answered', '  status:: answered\n  collapsed:: true'))
    const auto = collapseAnsweredOnLoad(tree)
    // Still collapsed, but it's the file's own state — ours to keep persisting.
    const answered = [...tree.nodes.values()].find((n) => n.status === 'answered')!
    expect(answered.collapsed).toBe(true)
    expect(auto.size).toBe(0)
  })

  it('is a view default: serializing must not write collapsed:: into the note', () => {
    const tree = parseOutline(NOTE)
    const auto = collapseAnsweredOnLoad(tree)
    const out = serializeOutline(tree, new Set(), false, auto)
    expect(out).not.toContain('collapsed::')
    expect(out).toBe(serializeOutline(parseOutline(NOTE), new Set(), false))
  })

  it('still writes collapsed:: for a node the user collapsed themselves', () => {
    const tree = parseOutline(NOTE)
    const auto = collapseAnsweredOnLoad(tree)
    const plain = [...tree.nodes.values()].find((n) => n.content === '一条普通笔记')!
    plain.collapsed = true
    expect(serializeOutline(tree, new Set(), false, auto)).toContain('collapsed:: true')
  })
})
