import { describe, it, expect } from 'vitest'
import { parse as parseYaml } from 'yaml'
import { actor, isOkfActor, humanActorId, addVerified } from './actor'

describe('actor(§7)', () => {
  it('formats the three actor shapes', () => {
    expect(actor.human('ahormati')).toBe('human:ahormati')
    expect(actor.agent('claude-code', 'opus-5')).toBe('claude-code/opus-5')
    expect(actor.process('finance-nightly')).toBe('process:finance-nightly')
  })

  it('recognises conformant actors and rejects a bare agent name', () => {
    expect(isOkfActor('human:bruce')).toBe(true)
    expect(isOkfActor('process:vault-sync')).toBe(true)
    expect(isOkfActor('claude-code/opus-5')).toBe(true)
    expect(isOkfActor('claude-code')).toBe(false)   // 缺版本段
    expect(isOkfActor('')).toBe(false)
    expect(isOkfActor('human:')).toBe(false)
  })
})

describe('humanActorId', () => {
  it('prefers the git email local part — stable and already unique', () => {
    expect(humanActorId({ name: 'Bruce Li', email: 'bruce@runningbruce.com', osUser: 'bruce' })).toBe('bruce')
  })
  it('falls back to a slug of the git name', () => {
    expect(humanActorId({ name: 'Bruce Li', email: '', osUser: 'x' })).toBe('bruce-li')
  })
  it('falls back to the OS user when git is unconfigured', () => {
    expect(humanActorId({ name: '', email: '', osUser: 'bruce' })).toBe('bruce')
  })
  it('never yields an empty id', () => {
    expect(humanActorId({ name: '', email: '', osUser: '' })).toBe('local')
  })
  it('keeps CJK names verbatim (file-over-app: no transliteration)', () => {
    expect(humanActorId({ name: '李雷', email: '', osUser: '' })).toBe('李雷')
  })
})

describe('addVerified(§5.2/§11)', () => {
  const AT = '2026-08-04T09:00:00.000Z'

  it('adds the first verification as a one-element list', () => {
    expect(addVerified(null, 'human:bruce', AT))
      .toBe(`verified:\n  - by: human:bruce\n    at: ${AT}`)
  })

  it('appends to an existing list', () => {
    const raw = 'type: Outline Note\nverified:\n  - by: process:nightly\n    at: 2026-01-01T00:00:00.000Z'
    const out = addVerified(raw, 'human:bruce', AT)
    expect(out).toContain('- by: process:nightly')
    expect(out).toContain('- by: human:bruce')
  })

  it('promotes a bare mapping to a list rather than dropping it (§11 MUST)', () => {
    const raw = 'verified: { by: process:nightly, at: 2026-01-01T00:00:00.000Z }'
    // 原有条目保持它自己的书写风格(流式),只是被搬进列表 —— 不重排用户的格式
    const parsed = parseYaml(addVerified(raw, 'human:bruce', AT))
    expect(parsed.verified.map((v: { by: string }) => v.by)).toEqual(['process:nightly', 'human:bruce'])
  })

  it('is idempotent for the same actor and timestamp', () => {
    const once = addVerified(null, 'human:bruce', AT)
    expect(addVerified(once, 'human:bruce', AT)).toBe(once)
  })

  it('leaves a non-mapping frontmatter untouched', () => {
    expect(addVerified('just prose', 'human:bruce', AT)).toBe('just prose')
  })
})
