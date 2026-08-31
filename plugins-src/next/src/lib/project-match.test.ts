import { describe, expect, it } from 'vitest'
import { buildProjectMatcher } from './project-match'

describe('local project matching', () => {
  const matcher = buildProjectMatcher([
    { projects: ['Next'], text: '念头分流 看板 泳道 WIP 关闭出口 项目标签 自动推荐' },
    { projects: ['插件市场'], text: '插件市场 安装 更新 下载 签名 索引 发布版本' },
    { projects: ['写作'], text: '文章 写作 草稿 编辑 发布 读者 标题' },
  ], ['Next', '插件市场', '写作'])

  it('prefers an explicit project-name mention without an LLM call', () => {
    expect(matcher.recommend('为 Next 增加一个新的卡片交互')).toMatchObject({ project: 'Next', reason: 'name' })
  })

  it('matches Chinese content through a prebuilt inverted index', () => {
    const result = matcher.recommend('优化插件下载和签名校验，发布新的市场索引')
    expect(result).toMatchObject({ project: '插件市场', reason: 'content' })
    expect(result!.matchedTerms.length).toBeGreaterThanOrEqual(2)
    expect(result!.candidatesScored).toBeLessThanOrEqual(2)
  })

  it('does not guess from one weak overlap or an ambiguous tie', () => {
    expect(matcher.recommend('准备发布')).toBeNull()
    const ambiguous = buildProjectMatcher([
      { projects: ['A'], text: '本地 缓存 同步 校验' },
      { projects: ['B'], text: '本地 缓存 同步 校验' },
    ], ['A', 'B'])
    expect(ambiguous.recommend('检查本地缓存同步')).toBeNull()
  })

  it('uses confirmed multi-project examples as training evidence', () => {
    const shared = buildProjectMatcher([
      { projects: ['Next', '写作'], text: '把 open idea 整理成完整文章并发布' },
      { projects: ['Next'], text: '念头 安放 泳道 关闭' },
    ], ['Next', '写作'])
    expect(shared.recommend('念头需要安放到正确泳道')).toMatchObject({ project: 'Next' })
  })
})
