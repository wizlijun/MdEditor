import { describe, it, expect } from 'vitest'
import { CATALOGS, LOCALES, setLocale, t } from './strings'

const en = CATALOGS.en
const enKeys = Object.keys(en)
const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

describe('t', () => {
  it('returns the English string for a known key by default', () => {
    expect(t('pickFile')).toBe('Choose Roam export (.zip / .json)…')
  })

  it('returns the localized string once the locale is set', () => {
    setLocale('zh')
    expect(t('pickFile')).toBe('选择 Roam 导出文件（.zip / .json）…')
    setLocale('ja')
    expect(t('pickFile')).toBe('Roam エクスポートを選択（.zip / .json）…')
    setLocale('de')
    expect(t('pickFile')).toBe('Roam-Export auswählen (.zip / .json)…')
    setLocale('en')
  })

  it('falls back to English for an unknown or absent locale', () => {
    setLocale('fr')
    expect(t('pickFile')).toBe('Choose Roam export (.zip / .json)…')
    setLocale(undefined)
    expect(t('pickFile')).toBe('Choose Roam export (.zip / .json)…')
  })

  it('interpolates placeholders', () => {
    setLocale('en')
    expect(t('progress', { done: 3, total: 10, current: 'Foo' })).toBe('3 / 10 pages — Foo')
    setLocale('zh')
    expect(t('progress', { done: 3, total: 10, current: 'Foo' })).toBe('3 / 10 页 — Foo')
    setLocale('en')
  })

  it('falls back to the raw key when the key is unknown', () => {
    expect(t('no.such.key' as never)).toBe('no.such.key')
  })

  it('has the Roam CLI sync strings in every locale', () => {
    const keys = [
      'cli.toggle', 'cli.link', 'cli.state.missing', 'cli.state.notConnected',
      'cli.state.ready', 'cli.probeFailed', 'cli.install', 'cli.connect', 'cli.date',
      'cli.graph', 'cli.sync',
      'cli.syncing', 'cli.result', 'cli.resultGoneKept', 'cli.noPage', 'cli.failed',
    ]
    for (const loc of LOCALES) {
      for (const k of keys) expect(CATALOGS[loc], `${loc}.${k}`).toHaveProperty(k)
    }
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
    // Everything must differ from English, except bare product/link names —
    // and terms Roam itself uses that are the same word in the target language
    // (mirrors ebook-import's strings.test.ts). `Graph` is Roam's own name for
    // a database and is the German word too; the existing `cli.state.ready`
    // string already renders it that way in de.
    const allowed = new Set(['cli.link', ...(locale === 'de' ? ['cli.graph'] : [])])
    const identical = enKeys.filter(
      (k) =>
        !allowed.has(k) &&
        catalog[k as keyof typeof catalog] === en[k as keyof typeof en] &&
        /[a-zA-Z]{4}/.test(en[k as keyof typeof en]),
    )
    expect(identical, `untranslated in ${locale}`).toEqual([])
  })
})
