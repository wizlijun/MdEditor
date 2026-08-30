import { describe, expect, it } from 'vitest'
import { CATALOGS, LOCALES, setLocale, t } from './strings'

const englishKeys = Object.keys(CATALOGS.en)
const placeholders = (value: string) => (value.match(/\{\w+\}/g) ?? []).sort()

describe('Next strings', () => {
  it('keeps the product name unchanged in every locale', () => {
    for (const locale of LOCALES) expect(CATALOGS[locale]['app.title']).toBe('Next')
  })

  it('uses the selected locale and interpolates values', () => {
    setLocale('zh-CN')
    expect(t('count.wip', { count: 2 })).toBe('2/3')
    expect(t('sheet.title', { title: '验证想法' })).toBe('安放“验证想法”')
    setLocale('en')
  })

  it('falls back to English for unsupported locales', () => {
    setLocale('fr')
    expect(t('app.value')).toBe(CATALOGS.en['app.value'])
    setLocale('en')
  })

  it.each(LOCALES)('%s catalog has the same keys and placeholders', (locale) => {
    expect(Object.keys(CATALOGS[locale]).sort()).toEqual(englishKeys.slice().sort())
    for (const key of englishKeys) {
      expect(CATALOGS[locale][key as keyof typeof CATALOGS.en]).toBeTruthy()
      expect(placeholders(CATALOGS[locale][key as keyof typeof CATALOGS.en])).toEqual(
        placeholders(CATALOGS.en[key as keyof typeof CATALOGS.en]),
      )
    }
  })
})
