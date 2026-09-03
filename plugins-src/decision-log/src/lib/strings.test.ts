import { describe, it, expect } from 'vitest'
import manifest from '../../manifest.v2.json'
import { CATALOGS, LOCALES, setLocale, t, starLabel } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('product name', () => {
  it('uses one localized name across the manifest and window UI', () => {
    const names = {
      en: 'Decision',
      zh: '决策',
      ja: '意思決定',
      de: 'Entscheidung',
    } as const

    expect(manifest.name).toBe(names.en)
    expect(manifest.contributes.menus[0]?.label).toBe(names.en)
    expect(manifest.contributes.windows[0]?.title).toBe(names.en)

    for (const locale of ['zh', 'ja', 'de'] as const) {
      expect(manifest.i18n[locale].name).toBe(names[locale])
      expect(manifest.i18n[locale].menus.open).toBe(names[locale])
    }
    for (const locale of LOCALES) {
      expect(CATALOGS[locale]['panel.title']).toBe(names[locale])
    }
  })
})

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

  it('falls back to the raw key when the key is unknown', () => {
    expect(t('no.such.key' as never)).toBe('no.such.key')
  })
})

describe('starLabel', () => {
  it('clamps to the 1..5 range and localizes', () => {
    setLocale('en')
    expect(starLabel(0)).toBe(t('conf.s1'))
    expect(starLabel(1)).toBe(t('conf.s1'))
    expect(starLabel(5)).toBe(t('conf.s5'))
    expect(starLabel(9)).toBe(t('conf.s5'))
    setLocale('zh')
    expect(starLabel(3)).toBe('挺有把握')
    setLocale('en')
  })
})

// A plugin window can't import the host's i18n, so nothing but this test stops
// a half-translated catalog from shipping — the gaps would surface as English
// mixed into a localized UI. (The catalogs are typed `Record<MessageKey, string>`,
// not `Partial`, so a genuinely missing key is already a compile error; this
// test guards content quality: non-empty, no stray keys, matching placeholders,
// and no leftover-English copy.)
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
    // 'sugg.detail' is a genuine German cognate ("Details" is standard German
    // UI vocabulary, not a missed translation) — everything else must differ.
    const allowed = new Set<string>(locale === 'de' ? ['sugg.detail'] : [])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
