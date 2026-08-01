import { describe, it, expect } from 'vitest'
import { CATALOGS, LOCALES, t } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('t', () => {
  it('returns the English string for a known key', () => {
    expect(t('en', 'run.start')).toBe('Run')
  })

  it('returns the localized string for the active locale', () => {
    expect(t('zh', 'run.start')).toBe('运行')
    expect(t('ja', 'run.start')).toBe('実行')
    expect(t('de', 'run.start')).toBe('Ausführen')
  })

  it('accepts a locale with a region suffix', () => {
    expect(t('zh-CN', 'run.start')).toBe('运行')
  })

  it('falls back to English for an unknown locale', () => {
    expect(t('fr', 'run.start')).toBe('Run')
    expect(t('', 'run.start')).toBe('Run')
  })

  it('interpolates placeholders', () => {
    expect(t('en', 'turns', { n: 3 })).toBe('3 turns')
    expect(t('zh', 'turns', { n: 3 })).toBe('3 轮')
  })

  it('falls back to the raw key when the key is unknown', () => {
    expect(t('en', 'no.such.key' as never)).toBe('no.such.key')
  })
})

// A plugin window can't import the host's i18n, so nothing but this test stops
// a half-translated catalog from shipping — the gaps would surface as English
// mixed into a Chinese UI.
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
    // Product names are the same in every language; everything else must differ.
    const allowed = new Set(['artifacts.label'])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
