import { describe, it, expect } from 'vitest'
import { planNoteHome, noteHomeForRead } from './note-home'
import type { SotRecord } from '../sotvault-logic'

const rec = (source_path: string, vault_path: string): SotRecord => ({
  vault_path, source_path, synced_at: 1, source_hash: 'a', vault_hash: 'b',
})

describe('planNoteHome', () => {
  it('(a) keeps a legacy sidecar note in place, no sync', () => {
    const p = planNoteHome('/dl/foo.md', { vaultRoot: '/v', records: [], legacyNoteExists: true })
    expect(p).toEqual({ action: 'use', notePath: '/dl/foo.note.md' })
  })
  it('(b) synced source → note next to the vault copy', () => {
    const records = [rec('/dl/foo.md', '/v/Sync/2026-07-15-foo.md')]
    const p = planNoteHome('/dl/foo.md', { vaultRoot: '/v', records, legacyNoteExists: false })
    expect(p).toEqual({ action: 'use', notePath: '/v/Sync/2026-07-15-foo.note.md' })
  })
  it('vault record WINS over a stray source-side note (no re-pollution)', () => {
    const records = [rec('/dl/foo.md', '/v/Sync/2026-07-15-foo.md')]
    // even if a legacy .note.md lingers next to the source, a synced record routes to vault
    const p = planNoteHome('/dl/foo.md', { vaultRoot: '/v', records, legacyNoteExists: true })
    expect(p).toEqual({ action: 'use', notePath: '/v/Sync/2026-07-15-foo.note.md' })
  })
  it('(c) file already under vault → note beside it', () => {
    const p = planNoteHome('/v/Sync/x.md', { vaultRoot: '/v', records: [], legacyNoteExists: false })
    expect(p).toEqual({ action: 'use', notePath: '/v/Sync/x.note.md' })
  })
  it('(d) outside vault, unsynced, no legacy note, vault configured → sync', () => {
    const p = planNoteHome('/dl/foo.md', { vaultRoot: '/v', records: [], legacyNoteExists: false })
    expect(p).toEqual({ action: 'sync' })
  })
  it('(d) no vault configured → configure-vault', () => {
    const p = planNoteHome('/dl/foo.md', { vaultRoot: null, records: [], legacyNoteExists: false })
    expect(p).toEqual({ action: 'configure-vault' })
  })
})

describe('noteHomeForRead', () => {
  it('maps a synced source to its vault companion', () => {
    const records = [rec('/dl/foo.md', '/v/Sync/2026-07-15-foo.md')]
    expect(noteHomeForRead('/dl/foo.md', { vaultRoot: '/v', records }))
      .toBe('/v/Sync/2026-07-15-foo.note.md')
  })
  it('falls back to the source companion when unsynced (legacy/vault-internal/empty)', () => {
    expect(noteHomeForRead('/dl/foo.md', { vaultRoot: '/v', records: [] }))
      .toBe('/dl/foo.note.md')
  })
  it('a note path itself has no companion — callers get null, not a double suffix', () => {
    const records = [rec('/dl/foo.md', '/v/Sync/2026-07-15-foo.md')]
    expect(noteHomeForRead('/dl/foo.note.md', { vaultRoot: '/v', records })).toBeNull()
  })
})
