import { describe, expect, it } from 'vitest'
import { localizedPluginDescription, localizedPluginName } from './plugin-text'

const plugin = {
  id: 'notemd.idea-spark',
  name: 'Idea Spark',
  description: 'Capture a spark.',
  i18n: {
    zh: { name: '奇思妙想', description: '记下一闪而过的灵感。' },
    ja: { name: 'アイデアスパーク', description: 'ひらめきを記録します。' },
    de: { name: 'Ideenfunke', description: 'Einen Einfall festhalten.' },
  },
}

describe('plugin market localized text', () => {
  it('uses the English base text in English', () => {
    expect(localizedPluginName(plugin, 'en')).toBe('Idea Spark')
    expect(localizedPluginDescription(plugin, 'en')).toBe('Capture a spark.')
  })

  it('appends the English product name when the localized name is not Western text', () => {
    expect(localizedPluginName(plugin, 'zh')).toBe('奇思妙想（Idea Spark）')
    expect(localizedPluginName(plugin, 'ja')).toBe('アイデアスパーク（Idea Spark）')
    expect(localizedPluginName(plugin, 'de')).toBe('Ideenfunke')
    expect(localizedPluginName({ ...plugin, i18n: { zh: { name: 'Claude 智能体' } } }, 'zh'))
      .toBe('Claude 智能体（Idea Spark）')
  })

  it('does not repeat a product name that remains English', () => {
    expect(localizedPluginName({ ...plugin, i18n: { zh: { name: 'Idea Spark' } } }, 'zh'))
      .toBe('Idea Spark')
  })

  it('falls back per field and accepts regional locale tags', () => {
    const partial = {
      ...plugin,
      i18n: { zh: { name: '奇思妙想' } },
    }
    expect(localizedPluginName(partial, 'zh-CN')).toBe('奇思妙想（Idea Spark）')
    expect(localizedPluginDescription(partial, 'zh-CN')).toBe('Capture a spark.')
  })

  it('ignores blank or malformed localized values', () => {
    const malformed = {
      ...plugin,
      i18n: {
        zh: { name: '  ', description: '' },
        ja: 'bad',
      },
    }
    expect(localizedPluginName(malformed, 'zh')).toBe('Idea Spark')
    expect(localizedPluginDescription(malformed, 'zh')).toBe('Capture a spark.')
    expect(localizedPluginName(malformed, 'ja')).toBe('Idea Spark')
  })

  it('uses the plugin id when every name is absent', () => {
    expect(localizedPluginName({ id: 'notemd.unknown', name: null }, 'zh')).toBe('notemd.unknown')
    expect(localizedPluginDescription({ id: 'notemd.unknown', name: null }, 'zh')).toBeNull()
  })
})
