import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock the Tauri store plugin used for persistence.
const storeGet = vi.fn()
const storeSet = vi.fn()
const storeSave = vi.fn()
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: storeGet, set: storeSet, save: storeSave })) },
}))

import { i18n, t, setLocale, loadLocale, availableLocales } from './store.svelte'
import { en } from './en'
import { zh } from './zh'
import { ja } from './ja'
import { de } from './de'

const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()

beforeEach(() => {
  storeGet.mockReset(); storeSet.mockReset(); storeSave.mockReset()
  i18n.locale = 'en'
})

describe('t', () => {
  it('returns the English string for a known key', () => {
    expect(t('folderView.reveal')).toBe('Reveal in Finder')
  })
  it('interpolates {name} placeholders from params', () => {
    expect(t('time.minutesAgo', { n: 5 })).toBe('5 min ago')
  })
  it('leaves a placeholder untouched when no matching param is given', () => {
    expect(t('time.minutesAgo')).toBe('{n} min ago')
  })
  it('falls back to the raw key when the key is unknown', () => {
    // @ts-expect-error — intentionally passing a key outside the catalog
    expect(t('does.not.exist')).toBe('does.not.exist')
  })
})

describe('availableLocales', () => {
  it('includes English, Simplified Chinese and Japanese', () => {
    const codes = availableLocales.map((l) => l.code)
    expect(codes).toEqual(expect.arrayContaining(['en', 'zh', 'ja']))
  })
})

// Every catalog in `availableLocales` belongs here — German was shipping as a
// selectable language while nothing checked it for gaps.
describe.each([
  ['zh', zh],
  ['ja', ja],
  ['de', de],
])('%s catalog', (_name, catalog) => {
  const enKeys = Object.keys(en) as (keyof typeof en)[]

  it('translates every English key to a non-empty string', () => {
    for (const key of enKeys) {
      expect(catalog[key], `missing key: ${key}`).toBeTruthy()
    }
  })

  it('has no keys beyond the English catalog', () => {
    expect(Object.keys(catalog).sort()).toEqual(enKeys.slice().sort())
  })

  it('preserves the same {placeholders} as English', () => {
    for (const key of enKeys) {
      expect(placeholders(catalog[key]), `placeholder mismatch: ${key}`)
        .toEqual(placeholders(en[key]))
    }
  })
})

describe('t with a non-English locale', () => {
  it('returns the localized string for the active locale', () => {
    i18n.locale = 'zh'
    expect(t('folderView.reveal')).toBe('在访达中显示')
    i18n.locale = 'ja'
    expect(t('folderView.reveal')).toBe('Finder で表示')
    i18n.locale = 'en'
  })
})

describe('loadLocale', () => {
  it('hydrates a valid stored locale', async () => {
    storeGet.mockResolvedValue('en')
    await loadLocale()
    expect(i18n.locale).toBe('en')
  })
  it('falls back to English for an unknown stored value', async () => {
    storeGet.mockResolvedValue('zz')
    await loadLocale()
    expect(i18n.locale).toBe('en')
  })
  it('falls back to English when nothing is stored', async () => {
    storeGet.mockResolvedValue(undefined)
    await loadLocale()
    expect(i18n.locale).toBe('en')
  })
})

describe('setLocale', () => {
  it('sets and persists a valid locale', async () => {
    await setLocale('en')
    expect(i18n.locale).toBe('en')
    expect(storeSet).toHaveBeenCalledWith('locale', 'en')
    expect(storeSave).toHaveBeenCalled()
  })
  it('ignores an invalid locale', async () => {
    // @ts-expect-error — intentionally passing an unsupported code
    await setLocale('zz')
    expect(storeSet).not.toHaveBeenCalled()
  })
})

// Task B-T8 review round 1: `search.index.tiersHint` (the settings page's
// "raw source material looks high → add frontmatter" sentence, design spec
// §9) quotes each locale's OWN `search.group.source` label in prose, by
// design — but nothing before this test pinned the two together. Renaming
// `search.group.source` would silently leave all four hint strings
// referring to a label that no longer appears on screen, with no red test
// to catch it.
describe.each([
  ['en', en],
  ['zh', zh],
  ['ja', ja],
  ['de', de],
])('%s search.index.tiersHint', (_name, catalog) => {
  it("quotes this locale's own search.group.source label", () => {
    expect(catalog['search.index.tiersHint']).toContain(catalog['search.group.source'])
  })
})
