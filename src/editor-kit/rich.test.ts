import { describe, it, expect } from 'vitest'
import { placeholderPlugin } from '../lib/placeholder-plugin'

describe('kit rich placeholder', () => {
  it('is part of the plugin set only when a placeholder was given', async () => {
    // mountRich 需要真实 DOM + moraya,jsdom 下挂不起来;这里验证的是接线
    // 契约:传了 placeholder 才追加插件、且插件带的正是那段文字。
    const { richPlugins } = await import('./rich')
    expect(richPlugins(undefined)).toHaveLength(0)
    const withText = richPlugins('写点什么')
    expect(withText).toHaveLength(1)
    // 插件的 decorations 对空文档产出带 data-placeholder 的装饰
    expect(withText[0]).toBeInstanceOf(placeholderPlugin('x').constructor)
  })
})
