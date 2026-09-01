import { describe, it, expect } from 'vitest'
import { deriveAnswers, answeredByNoteText } from './answers'
import { parseOutline } from './markdown'

const note = [
  '- 原文一',
  '  type:: annotation',
  '  line:: 3',
  '  - 为什么能到 90%?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为前缀重复。',
  '      ```',
  '      type:: answer',
  '      by:: claude-code',
  '  - 还没答的问题?',
  '    type:: question',
  '    status:: open',
  '- 原文二',
  '  type:: annotation',
  '  line:: 9',
  '  - 已采纳的问题?',
  '    type:: question',
  '    status:: adopted',
  '    - ```markdown',
  '      旧答复。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

/** 多节点形态:答复节点写结论,论点分列成子节点(agent 会自带 ✦ 前缀) */
const multiNote = [
  '- 原文',
  '  type:: annotation',
  '  line:: 3',
  '  - 持有开环的成本?',
  '    type:: question',
  '    status:: answered',
  '    - ✦ 不需要"真的完成"。',
  '      type:: answer',
  '      by:: claude-code/opus-5',
  '      - ✦ 侵入性思维:托管出去就消失。',
  '        - ✦ 更深一层的论据',
  '      - ✦ 注意力残留:只有明确关闭才最小。',
  '',
].join('\n')

describe('answer body assembly', () => {
  const bodyOf = (src: string) => deriveAnswers(parseOutline(src))[0].body

  it('renders answer child nodes as a nested markdown list', () => {
    expect(bodyOf(multiNote)).toBe([
      '不需要"真的完成"。',
      '',
      '- 侵入性思维:托管出去就消失。',
      '  - 更深一层的论据',
      '- 注意力残留:只有明确关闭才最小。',
    ].join('\n'))
  })

  it('keeps the single fenced node form working', () => {
    expect(bodyOf(note)).toBe('因为前缀重复。')
  })

  it('emits list only when the answer node itself carries no text', () => {
    const src = multiNote.replace('- ✦ 不需要"真的完成"。', '- ')
    expect(bodyOf(src)).toBe([
      '- 侵入性思维:托管出去就消失。',
      '  - 更深一层的论据',
      '- 注意力残留:只有明确关闭才最小。',
    ].join('\n'))
  })

  it('indents continuation lines of a multi-line child to the item content column', () => {
    const src = [
      '- 原文',
      '  type:: annotation',
      '  - 问?',
      '    type:: question',
      '    status:: answered',
      '    - ✦ 结论',
      '      type:: answer',
      '      - ```js',
      '        const a = 1',
      '',
      '        const b = 2',
      '        ```',
      '',
    ].join('\n')
    // 围栏子节点的空行是语义(段落分隔),续行整体缩进到条目内容列
    expect(bodyOf(src)).toBe('结论\n\n- ```js\n  const a = 1\n\n  const b = 2\n  ```')
  })
})

describe('deriveAnswers', () => {
  it('returns one entry per question that has an answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.map(r => r.noteText).sort())
      .toEqual(['为什么能到 90%?', '已采纳的问题?'].sort())
  })

  it('carries body (fence stripped), status and author', () => {
    const rows = deriveAnswers(parseOutline(note))
    const r = rows.find(x => x.noteText === '为什么能到 90%?')!
    expect(r.body).toBe('因为前缀重复。')
    expect(r.status).toBe('answered')
    expect(r.by).toBe('claude-code')
    expect(r.questionId).toBeTruthy()
  })

  it('skips questions with no answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.some(r => r.noteText === '还没答的问题?')).toBe(false)
  })

  it('answeredByNoteText only exposes status answered', () => {
    const map = answeredByNoteText(deriveAnswers(parseOutline(note)))
    expect(map.has('为什么能到 90%?')).toBe(true)
    expect(map.has('已采纳的问题?')).toBe(false)   // adopted 不再出卡片
  })

  it('keeps duplicate question texts as separate ordered answers', () => {
    const duplicate = [
      '- 原文一',
      '  type:: annotation',
      '  - 为什么?',
      '    type:: question',
      '    status:: answered',
      '    - 第一条答复',
      '      type:: answer',
      '- 原文二',
      '  type:: annotation',
      '  - 为什么?',
      '    type:: question',
      '    status:: answered',
      '    - 第二条答复',
      '      type:: answer',
      '',
    ].join('\n')

    const rows = deriveAnswers(parseOutline(duplicate))
    expect(rows.map(r => [r.questionOccurrence, r.body])).toEqual([
      [0, '第一条答复'],
      [1, '第二条答复'],
    ])
    expect(answeredByNoteText(rows).get('为什么?')?.map(r => r.body))
      .toEqual(['第一条答复', '第二条答复'])
  })

  it('preserves occurrence gaps when an earlier duplicate has no visible answer', () => {
    const duplicate = [
      '- 原文一',
      '  type:: annotation',
      '  - 为什么?',
      '    type:: question',
      '    status:: open',
      '- 原文二',
      '  type:: annotation',
      '  - 为什么?',
      '    type:: question',
      '    status:: answered',
      '    - 第二条答复',
      '      type:: answer',
      '',
    ].join('\n')

    const answers = answeredByNoteText(deriveAnswers(parseOutline(duplicate))).get('为什么?')!
    expect(answers[0]).toBeUndefined()
    expect(answers[1].body).toBe('第二条答复')
  })
})
