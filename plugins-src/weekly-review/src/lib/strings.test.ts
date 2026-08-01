import { describe, it, expect } from 'vitest'
import { CATALOGS, LOCALES, setLocale, t } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('t', () => {
  it('returns the English string for a known key by default', () => {
    expect(t('title')).toBe('Weekly Review')
  })

  it('returns the localized string once the locale is set', () => {
    setLocale('zh')
    expect(t('title')).toBe('周检视')
    setLocale('ja')
    expect(t('title')).toBe('ウィークリーレビュー')
    setLocale('de')
    expect(t('title')).toBe('Wochenrückblick')
    setLocale('en')
  })

  it('accepts a locale with a region suffix', () => {
    setLocale('zh-CN')
    expect(t('title')).toBe('周检视')
    setLocale('en')
  })

  it('falls back to English for an unknown or absent locale', () => {
    setLocale('fr')
    expect(t('title')).toBe('Weekly Review')
    setLocale(undefined)
    expect(t('title')).toBe('Weekly Review')
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
    // 'month.suffix' is intentionally '' in English (and German) — no
    // "of the month" suffix in those locales' date conventions — so an
    // empty English value doesn't require a non-empty translation.
    for (const key of enKeys) {
      if (en[key as keyof typeof en] === '') continue
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
    // 'month.suffix' is deliberately empty in en/de (no "of the month" suffix
    // in those locales' date conventions) — everything else must differ.
    // Single-letter weekday abbreviations (dow.*) are exempt by the length
    // guard below, not by this allow-list.
    const allowed = new Set<string>([])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
