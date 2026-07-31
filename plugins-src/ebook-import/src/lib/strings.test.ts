import { describe, it, expect } from 'vitest'
import { CATALOGS, LOCALES, setLocale, t } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('t', () => {
  it('returns the English string for a known key by default', () => {
    expect(t('action.cancel')).toBe('Cancel')
  })

  it('returns the localized string once the locale is set', () => {
    setLocale('zh')
    expect(t('action.cancel')).toBe('取消')
    setLocale('ja')
    expect(t('action.cancel')).toBe('キャンセル')
    setLocale('de')
    expect(t('action.cancel')).toBe('Abbrechen')
    setLocale('en')
  })

  it('accepts a locale with a region suffix', () => {
    setLocale('zh-CN')
    expect(t('action.cancel')).toBe('取消')
    setLocale('en')
  })

  it('falls back to English for an unknown or absent locale', () => {
    setLocale('fr')
    expect(t('action.cancel')).toBe('Cancel')
    setLocale(undefined)
    expect(t('action.cancel')).toBe('Cancel')
  })

  it('interpolates placeholders', () => {
    setLocale('en')
    expect(t('settings.calibre.found', { path: '/usr/bin/ebook-convert', version: '7.1' })).toBe(
      'Calibre found: /usr/bin/ebook-convert (7.1)',
    )
    setLocale('zh')
    expect(t('settings.calibre.found', { path: '/usr/bin/ebook-convert', version: '7.1' })).toBe(
      '已找到 Calibre：/usr/bin/ebook-convert（7.1）',
    )
    setLocale('en')
  })

  it('falls back to the raw key when the key is unknown', () => {
    expect(t('no.such.key' as never)).toBe('no.such.key')
  })
})

// A plugin window can't import the host's i18n, so nothing but this test stops
// a half-translated catalog from shipping — the gaps would surface as English
// mixed into a localized UI.
describe.each(LOCALES.filter((l) => l !== 'en'))('%s catalog', (locale) => {
  const catalog = CATALOGS[locale]

  it('translates every English key to a non-empty string', () => {
    for (const key of enKeys) {
      expect(catalog[key as keyof typeof catalog], `missing key: ${key}`).toBeTruthy()
    }
  })

  it('has no keys beyond the English catalog', () => {
    expect(Object.keys(catalog).sort()).toEqual(enKeys.slice().sort())
  })

  it('preserves the same {placeholders} as English', () => {
    for (const key of enKeys) {
      expect(
        placeholders(catalog[key as keyof typeof catalog] ?? ''),
        `placeholder mismatch: ${key}`,
      ).toEqual(placeholders(en[key as keyof typeof en]))
    }
  })

  it('leaves no string still in English', () => {
    // Product/provider names are the same in every language; everything else
    // must differ.
    const allowed = new Set(['ocr.provider.wechat', 'ocr.provider.baidu'])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
