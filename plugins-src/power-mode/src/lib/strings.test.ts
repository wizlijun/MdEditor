import { describe, it, expect } from 'vitest'
import { CATALOGS, t, setLocale, type MessageKey } from './strings'

describe('strings', () => {
  it('has every key in all four locales', () => {
    const keys = Object.keys(CATALOGS.en) as MessageKey[]
    expect(keys.length).toBeGreaterThan(10)
    for (const locale of ['zh', 'ja', 'de'] as const) {
      for (const k of keys) {
        expect(CATALOGS[locale][k], `${locale} missing ${k}`).toBeTruthy()
      }
    }
  })

  it('falls back to en for an unknown locale', () => {
    setLocale('fr')
    expect(t('title')).toBe(CATALOGS.en.title)
    setLocale('zh')
    expect(t('title')).toBe(CATALOGS.zh.title)
  })
})
