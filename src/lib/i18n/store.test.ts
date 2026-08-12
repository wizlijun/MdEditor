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

// Task B-T8 review round 1: `search.index.tiersHint` (the paragraph under the
// provenance-tier table, design spec §9) quotes each locale's OWN tier labels
// in prose, by design — but nothing before this test pinned them together.
// Renaming a `search.group.*` key would silently leave all four hint strings
// referring to a label that no longer appears on screen, with no red test
// to catch it.
//
// Final fix wave, Blocker 1 — WHAT the sentence says, not just which words it
// quotes. Until this wave it read "if 'Raw source material' looks unexpectedly
// high, it usually just means notes are missing frontmatter; add a `type:`
// field". Design spec §1 quotes that sentence verbatim as the defect this
// branch exists to fix: under rule 6′ a frontmatter-less file classifies
// `Unlabeled`, NEVER `Source`, so both halves became false — a high `Source`
// count now means the user's source globs are too wide, and the add-frontmatter
// advice belongs to the `Unlabeled` row it is no longer attached to. The
// branch fixed the label and left the hint, and no per-task reviewer could
// catch it because none of the 25 commits touched the string.
//
// Three relationships are pinned per locale, all of them things a future edit
// could plausibly break back:
//   1. the paragraph names BOTH tiers whose rules it now explains (5′ and 6′);
//   2. the add-frontmatter advice lives on `tiersUnlabeledHint` — the string
//      rendered against the `Unlabeled` row — and NOT on `tiersHint`, so the
//      two paragraphs cannot drift back into saying the same thing at the
//      wrong tier. `type:` is the token to key on: it is a literal frontmatter
//      key, identical in all four locales, and it appears in exactly the
//      sentence that gives the advice;
//   3. neither paragraph prescribes a full rebuild. Editing a file's
//      frontmatter changes its bytes, so the ordinary sweep/watcher re-runs
//      `chunk::parse_file` -> `origin::derive` for that file on its own; a
//      rebuild (search unavailable for ~10s, `search.rebuild`) is never
//      required to reclassify anything, and §7.4 makes this the ONE designed
//      exit from the ×0.3 demotion — prescribing the most disruptive action
//      available is bad advice at the exact moment the user is fixing
//      something. Each locale contributes the word stem its own "rebuild"
//      wording is built from, because there is no shared token to look for.
describe.each([
  ['en', en, /rebuild/i],
  ['zh', zh, /重建/],
  ['ja', ja, /再構築/],
  ['de', de, /neu erstell|erstell.{0,12}neu/i],
])('%s search.index.tiersHint', (_name, catalog, rebuildWording) => {
  it("quotes this locale's own search.group.source label", () => {
    expect(catalog['search.index.tiersHint']).toContain(catalog['search.group.source'])
  })
  it("quotes this locale's own search.group.unlabeled label", () => {
    // Rule 6′ is half of what this paragraph now explains. Before the fix
    // wave the paragraph never mentioned the tier at all — it attributed
    // rule 6′'s symptom to the `Source` row instead.
    expect(catalog['search.index.tiersHint']).toContain(catalog['search.group.unlabeled'])
  })
  it('leaves the add-frontmatter advice to the unlabeled-row hint', () => {
    expect(catalog['search.index.tiersUnlabeledHint']).toContain('type:')
    expect(catalog['search.index.tiersHint']).not.toContain('type:')
  })
  it('neither paragraph tells the user to rebuild the index', () => {
    expect(catalog['search.index.tiersHint']).not.toMatch(rebuildWording)
    expect(catalog['search.index.tiersUnlabeledHint']).not.toMatch(rebuildWording)
  })
  it('the stem it forbids is the one that locale really uses for rebuilding', () => {
    // Guards the test above from going vacuous: a `search.rebuild` rewording
    // that no longer matches this pattern must fail here rather than silently
    // stop checking anything.
    expect(catalog['search.rebuild']).toMatch(rebuildWording)
  })
})
