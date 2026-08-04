import { describe, it, expect } from 'vitest'
import { trustTier, lifecycleOf } from './trust'

describe('trustTier(§5.3,派生而非存储)', () => {
  it('is unverified without a verified key', () => {
    expect(trustTier('type: Note')).toBe('unverified')
    expect(trustTier(null)).toBe('unverified')
  })

  it('is machine-confirmed when only non-human actors verified', () => {
    expect(trustTier('verified:\n  - by: process:nightly\n    at: 2026-01-01T00:00:00Z'))
      .toBe('machine-confirmed')
    expect(trustTier('verified:\n  - by: claude-code/opus-5\n    at: 2026-01-01T00:00:00Z'))
      .toBe('machine-confirmed')
  })

  it('is human-reviewed as soon as one human: actor appears', () => {
    expect(trustTier('verified:\n  - by: process:nightly\n    at: x\n  - by: human:bruce\n    at: y'))
      .toBe('human-reviewed')
  })

  it('treats a bare mapping as a one-element list (§11 MUST)', () => {
    expect(trustTier('verified: { by: human:bruce, at: 2026-01-01T00:00:00Z }')).toBe('human-reviewed')
  })

  it('never throws on malformed front-matter — it just means unknown', () => {
    expect(trustTier('type: [unclosed')).toBe('unverified')
    expect(trustTier('- a list')).toBe('unverified')
    expect(trustTier('verified: 42')).toBe('unverified')
  })
})

describe('lifecycleOf(§5.4)', () => {
  it('defaults to stable when status is absent', () => {
    expect(lifecycleOf('type: Note').status).toBe('stable')
  })

  it('reads the declared status', () => {
    expect(lifecycleOf('status: draft').status).toBe('draft')
    expect(lifecycleOf('status: deprecated').status).toBe('deprecated')
  })

  it('ignores a status value outside the vocabulary', () => {
    expect(lifecycleOf('status: 随便写的').status).toBe('stable')
  })

  it('reports staleness against the given day', () => {
    expect(lifecycleOf('stale_after: 2026-09-23', '2026-09-22').stale).toBe(false)
    expect(lifecycleOf('stale_after: 2026-09-23', '2026-09-23').stale).toBe(true)
    expect(lifecycleOf('type: Note', '2026-09-23').stale).toBe(false)
  })
})
