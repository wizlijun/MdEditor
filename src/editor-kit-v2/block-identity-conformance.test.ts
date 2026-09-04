import { describe, expect, it } from 'vitest'
import { createSchema } from '@moraya/core'
import { materializeBlocks, serializeSpan } from './block-layout'
import { computeFingerprint } from '../lib/blockchunk/fingerprint'
import { mergeBlocks, type MergeOutcome } from '../lib/blockchunk/merge'
import { chunkDocumentSemantic, type SemanticChunkOptions } from '../lib/blockchunk/semantic-chunker'

async function reconcileIdentity(
  previous: readonly { id: string; text: string }[],
  nextMarkdown: string,
  options: SemanticChunkOptions,
): Promise<{ chunks: ReturnType<typeof chunkDocumentSemantic>; outcome: MergeOutcome }> {
  const chunks = chunkDocumentSemantic(nextMarkdown, options)
  const oldBlocks = await Promise.all(previous.map(async (block) => ({
    id: block.id,
    fp: await computeFingerprint(block.text),
  })))
  const newBlocks = await Promise.all(chunks.map(async (block) => ({
    fp: await computeFingerprint(block.text),
  })))
  return { chunks, outcome: mergeBlocks(oldBlocks, newBlocks) }
}

const sectionOptions = { cutLevel: 2, minChars: 0, maxChars: 9999 }
const schema = createSchema({
  mediaResolver: {
    loadLocalImage: async (path: string) => path,
    loadLocalMedia: async (path: string) => path,
    loadRemoteMedia: async (url: string) => url,
  },
})

describe('CDR conformance against production BlockYaml chunk/fingerprint/merge functions', () => {
  it('carries a production semantic chunk through the multi-node editor layout seam', () => {
    const markdown = '## 背景\n\n这里保留完整叙事。\n\n- 约束一\n- 约束二'
    const chunks = chunkDocumentSemantic(markdown, sectionOptions)
    const materialized = materializeBlocks(
      chunks.map((chunk, index) => ({ blockId: `block-${index}`, markdown: chunk.text })),
      schema,
    )

    expect(chunks).toHaveLength(1)
    expect(materialized.layout.spans).toEqual([{ blockId: 'block-0', startIndex: 0, endIndex: 3 }])
    expect(serializeSpan(materialized.doc, materialized.layout.spans[0])).toBe(markdown)
  })

  it('preserves a long Chinese section ID across a small local rewrite', async () => {
    const before = '## 项目背景\n\n这个项目用于维护长期背景、术语、事实、约束和团队共识，并让人类与多个 Agent 在同一份叙事文档上协作。'
    const after = '## 项目背景\n\n这个项目用于维护长期背景、术语、事实、约束和团队共识，并让人类与多个 Agent 在同一份叙事文档上安全协作。'
    const result = await reconcileIdentity([{ id: 'block-background', text: before }], after, sectionOptions)

    expect(result.outcome.edited).toMatchObject([{ newIdx: 0, oldId: 'block-background' }])
    expect(result.outcome.fresh).toHaveLength(0)
    expect(result.outcome.retired).toHaveLength(0)
  })

  it('keeps exact section IDs with their content after reordering', async () => {
    const first = '## 术语\n\n这里定义足够长且明确的团队术语，确保重排时身份跟随内容而不是位置。'
    const second = '## 约束\n\n这里记录足够长且明确的执行约束，确保重排时身份跟随内容而不是位置。'
    const result = await reconcileIdentity([
      { id: 'block-terms', text: first },
      { id: 'block-constraints', text: second },
    ], `${second}\n${first}`, sectionOptions)

    expect(result.outcome.kept.map((item) => `${item.newIdx}:${item.oldId}`).sort()).toEqual([
      '0:block-constraints',
      '1:block-terms',
    ])
  })

  it('documents the current no-go: an edited short section loses its ID', async () => {
    const before = '## 状态\n\n待确认'
    const after = '## 状态\n\n已确认'
    const result = await reconcileIdentity([{ id: 'block-status', text: before }], after, sectionOptions)

    expect(result.outcome.edited).toHaveLength(0)
    expect(result.outcome.fresh).toEqual([{ newIdx: 0 }])
    expect(result.outcome.retired).toEqual([{ oldId: 'block-status', replacedBy: [] }])
  })

  it('documents the current no-go: defaults coalesce adjacent short sections', () => {
    const chunks = chunkDocumentSemantic('## A\n\n短内容。\n## B\n\n另一段短内容。')

    expect(chunks).toHaveLength(1)
    expect(chunks[0].text).toContain('## A')
    expect(chunks[0].text).toContain('## B')
  })
})
