import { describe, it, expect } from 'vitest'
import { CATALOGS, MESSAGE_KEYS } from './strings'
describe('strings', () => {
  it('every locale covers every key (no silent fallback)', () => {
    for (const locale of ['en', 'zh', 'ja', 'de'] as const) {
      for (const key of MESSAGE_KEYS) {
        expect(CATALOGS[locale][key], `${locale}.${key}`).toBeTruthy()
      }
    }
  })

  it('describes a proof as evidence progress, not completed work', () => {
    expect({
      en: CATALOGS.en.statusDone,
      zh: CATALOGS.zh.statusDone,
      ja: CATALOGS.ja.statusDone,
      de: CATALOGS.de.statusDone,
    }).toEqual({
      en: 'Proofed',
      zh: '已论证',
      ja: '論証済み',
      de: 'Durchargumentiert',
    })
  })
})
