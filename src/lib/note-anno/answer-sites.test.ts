import { describe, it, expect } from 'vitest'
import { Schema } from 'prosemirror-model'
import { collectCardSites } from './answer-sites'
import type { AnswerEntry } from '../outline/answers'

// 最小测试 schema:collectCardSites 只关心「文本上的 annotation mark」与「note_anchor 节点」。
// @moraya/core 的 createSchema 需要 MediaResolver,对纯函数单测过重,故本地造一个。
const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { group: 'block', content: 'inline*' },
    text: { group: 'inline' },
    note_anchor: { group: 'inline', inline: true, atom: true, attrs: { note: { default: '' } } },
  },
  marks: {
    annotation: { attrs: { note: { default: '' } } },
    strong: {},
  },
})

const entry = (noteText: string): AnswerEntry => ({
  noteText, status: 'answered', body: 'body', questionId: 'q1',
})

function docWithAnnotation(note: string) {
  const anno = schema.marks.annotation.create({ note })
  return schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('前 '), schema.text('被批注', [anno])]),
    schema.node('paragraph', null, [schema.text('后一段')]),
  ])
}

describe('collectCardSites', () => {
  it('anchors a card just after the block holding the annotation', () => {
    const doc = docWithAnnotation('为什么?')
    const sites = collectCardSites(doc, new Map([['为什么?', entry('为什么?')]]))
    expect(sites).toHaveLength(1)
    // 插入点 = 第一段之后 = 第一个段落节点的 nodeSize
    expect(sites[0].pos).toBe(doc.child(0).nodeSize)
  })

  it('ignores annotations with no matching answer', () => {
    const doc = docWithAnnotation('没人答过?')
    expect(collectCardSites(doc, new Map([['别的问题?', entry('别的问题?')]]))).toHaveLength(0)
  })

  it('emits one site for a note_anchor node', () => {
    const doc = schema.node('doc', null, [
      schema.node('paragraph', null, [
        schema.text('句子'), schema.nodes.note_anchor.create({ note: '这样对吗?' }),
      ]),
    ])
    const sites = collectCardSites(doc, new Map([['这样对吗?', entry('这样对吗?')]]))
    expect(sites).toHaveLength(1)
  })

  it('does not duplicate a card when the annotation spans several text nodes', () => {
    const anno = schema.marks.annotation.create({ note: '为什么?' })
    const strong = schema.marks.strong.create()
    const doc = schema.node('doc', null, [
      schema.node('paragraph', null, [
        schema.text('a', [anno]), schema.text('b', [anno, strong]),
      ]),
    ])
    expect(collectCardSites(doc, new Map([['为什么?', entry('为什么?')]]))).toHaveLength(1)
  })

  it('returns nothing when the index is empty', () => {
    expect(collectCardSites(docWithAnnotation('为什么?'), new Map())).toHaveLength(0)
  })
})
