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
})
