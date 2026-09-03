import { describe, expect, it } from 'vitest'
import { actionTone, canPlan, hasProblems, initialUser, normalizeUsers, progressPercent, reportCount } from './migration'
import type { MigrationReport } from './types'

const report = (patch: Partial<MigrationReport> = {}): MigrationReport => ({
  schema_version: 1,
  mode: 'incremental',
  dry_run: true,
  scanned: 4,
  eligible: 3,
  create: 1,
  update: 1,
  skip: 1,
  conflict: 0,
  blocked: 0,
  excluded_audio: 7,
  committed: 0,
  source_missing: 0,
  warnings: [],
  errors: [],
  items: [],
  ...patch,
})

describe('Hemory migration view model', () => {
  it('normalizes string and object users without duplicates', () => {
    const detection = { users: ['bruce', { id: 'bruce', label: 'Bruce' }, { id: 'team', label: 'Team' }] }
    expect(normalizeUsers(detection)).toEqual([
      { id: 'bruce', label: 'bruce' },
      { id: 'team', label: 'Team' },
    ])
    expect(initialUser({ users: ['bruce'] })).toBe('bruce')
    expect(initialUser(detection)).toBe('')
  })

  it('requires an explicit user and historical timezone only when needed', () => {
    const users = normalizeUsers({ users: ['a', 'b'] })
    expect(canPlan('/hemory', '', users, false, '')).toBe(false)
    expect(canPlan('/hemory', 'a', users, true, '')).toBe(false)
    expect(canPlan('/hemory', 'a', users, true, 'Asia/Taipei')).toBe(true)
  })

  it('maps report counts and problem states without treating excluded audio as failure', () => {
    expect(reportCount(report(), 'excluded')).toBe(7)
    expect(hasProblems(report())).toBe(false)
    expect(hasProblems(report({ conflict: 1 }))).toBe(true)
    expect(actionTone('blocked')).toBe('danger')
    expect(actionTone('source_missing')).toBe('warning')
  })

  it('clamps progress to a stable percentage', () => {
    expect(progressPercent(1, 4)).toBe(25)
    expect(progressPercent(8, 4)).toBe(100)
    expect(progressPercent(0, 0)).toBe(0)
  })
})
