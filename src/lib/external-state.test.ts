import { describe, it, expect } from 'vitest'
import { decide, type ExternalEvent, type TabSnapshot } from './external-state'

const fresh = (overrides: Partial<TabSnapshot> = {}): TabSnapshot => ({
  initialContent: 'A',
  currentContent: 'A',
  lastKnownMtime: 1000,
  lastKnownHash: 'h-A',
  externalState: 'fresh',
  mode: 'source',
  ...overrides,
})

const modifiedEvent = (mtime: number, hash: string, content: string): ExternalEvent =>
  ({ type: 'modified', snapshot: { mtime, hash, content } })

describe('decide', () => {
  it('clean tab + external modify → autoReload', () => {
    const d = decide(fresh(), modifiedEvent(2000, 'h-B', 'B'))
    expect(d).toEqual({ kind: 'autoReload', snapshot: { mtime: 2000, hash: 'h-B', content: 'B' } })
  })

  it('dirty tab + external modify → showChanged', () => {
    const d = decide(fresh({ currentContent: 'A-edited' }), modifiedEvent(2000, 'h-B', 'B'))
    expect(d).toEqual({ kind: 'showChanged', snapshot: { mtime: 2000, hash: 'h-B', content: 'B' } })
  })

  it('matching mtime+hash → ignore (self-write echo)', () => {
    const d = decide(fresh(), modifiedEvent(1000, 'h-A', 'A'))
    expect(d).toEqual({ kind: 'ignore' })
  })

  it('different mtime but identical hash → ignore (touch only)', () => {
    const d = decide(fresh(), modifiedEvent(9999, 'h-A', 'A'))
    expect(d).toEqual({ kind: 'ignore' })
  })

  it('delete event on fresh tab → showDeleted', () => {
    const d = decide(fresh(), { type: 'deleted' })
    expect(d).toEqual({ kind: 'showDeleted' })
  })

  it('delete event on already-deleted tab → ignore', () => {
    const d = decide(fresh({ externalState: 'deleted' }), { type: 'deleted' })
    expect(d).toEqual({ kind: 'ignore' })
  })

  it('modify on already-deleted tab (file was recreated) → showChanged when dirty', () => {
    const d = decide(
      fresh({ externalState: 'deleted', currentContent: 'A-edited' }),
      modifiedEvent(2000, 'h-B', 'B'),
    )
    expect(d).toEqual({ kind: 'showChanged', snapshot: { mtime: 2000, hash: 'h-B', content: 'B' } })
  })

  it('modify on already-deleted tab with same hash → autoReload (not ignore)', () => {
    // File was deleted then recreated with content equal to the pre-deletion
    // version. Hash matches lastKnownHash, but the state must still transition
    // back from 'deleted' so the banner clears.
    const d = decide(
      fresh({ externalState: 'deleted' }),
      modifiedEvent(2000, 'h-A', 'A'),
    )
    expect(d).toEqual({ kind: 'autoReload', snapshot: { mtime: 2000, hash: 'h-A', content: 'A' } })
  })

  it('clean rich-mode tab + external modify → showChanged (never silent autoReload)', () => {
    // Rich-mode editor maintains internal state we cannot smoothly resync.
    // Always surface the banner so the user explicitly chooses Reload vs.
    // Overwrite. Source mode keeps the autoReload fast-path.
    const d = decide(fresh({ mode: 'rich' }), modifiedEvent(2000, 'h-B', 'B'))
    expect(d).toEqual({ kind: 'showChanged', snapshot: { mtime: 2000, hash: 'h-B', content: 'B' } })
  })

  it('clean rich-mode tab recreated after deletion → still showChanged', () => {
    // Even when transitioning out of `deleted`, rich mode must not silently
    // swap content under the editor.
    const d = decide(
      fresh({ mode: 'rich', externalState: 'deleted' }),
      modifiedEvent(2000, 'h-A', 'A'),
    )
    expect(d).toEqual({ kind: 'showChanged', snapshot: { mtime: 2000, hash: 'h-A', content: 'A' } })
  })
})

describe('decide — outline note tabs (.note.md)', () => {
  it('clean outline-note tab auto-reloads even though its mode is rich', () => {
    // 大纲笔记由 OutlineEditor 渲染,树可随时从文本重建,没有 ProseMirror
    // "内部状态反向覆盖磁盘"的顾虑 → 干净时应走静默重载
    const d = decide(fresh({ mode: 'rich', isOutlineNote: true }), modifiedEvent(2000, 'h-B', 'B'))
    expect(d.kind).toBe('autoReload')
  })

  it('dirty outline-note tab still shows the banner (never silently overwrite)', () => {
    const d = decide(
      fresh({ mode: 'rich', isOutlineNote: true, currentContent: 'A-edited' }),
      modifiedEvent(2000, 'h-B', 'B'),
    )
    expect(d.kind).toBe('showChanged')
  })

  it('an ordinary rich tab is unaffected — still banner-only', () => {
    const d = decide(fresh({ mode: 'rich' }), modifiedEvent(2000, 'h-B', 'B'))
    expect(d.kind).toBe('showChanged')
  })

  it('self-write echo is still ignored for outline notes', () => {
    const d = decide(fresh({ mode: 'rich', isOutlineNote: true }), modifiedEvent(2000, 'h-A', 'A'))
    expect(d.kind).toBe('ignore')
  })
})
