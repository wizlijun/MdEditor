import { describe, it, expect } from 'vitest'
import { CATALOGS, LOCALES, setLocale, t } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('t', () => {
  it('returns the English string for a known key by default', () => {
    expect(t('common.cancel')).toBe('Cancel')
  })

  it('returns the localized string once the locale is set', () => {
    setLocale('zh')
    expect(t('common.cancel')).toBe('取消')
    setLocale('ja')
    expect(t('common.cancel')).toBe('キャンセル')
    setLocale('de')
    expect(t('common.cancel')).toBe('Abbrechen')
    setLocale('en')
  })

  it('accepts a locale with a region suffix', () => {
    setLocale('zh-CN')
    expect(t('common.cancel')).toBe('取消')
    setLocale('en')
  })

  it('falls back to English for an unknown or absent locale', () => {
    setLocale('fr')
    expect(t('common.cancel')).toBe('Cancel')
    setLocale(undefined)
    expect(t('common.cancel')).toBe('Cancel')
  })

  it('interpolates placeholders', () => {
    setLocale('en')
    expect(t('chat.expiresIn', { time: '01:59' })).toBe('Expires in 01:59')
    setLocale('zh')
    expect(t('chat.expiresIn', { time: '01:59' })).toBe('01:59 后过期')
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
    // "OpenClaw" is the product name and stays the same in every language.
    // "System" is a loanword German keeps verbatim (de only — the assertion
    // still catches it going untranslated in zh/ja). Everything else must
    // differ.
    const allowed = new Set(['chat.role.agent', ...(locale === 'de' ? ['chat.role.system'] : [])])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
