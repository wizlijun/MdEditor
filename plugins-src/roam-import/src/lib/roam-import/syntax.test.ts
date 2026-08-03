import { describe, it, expect } from 'vitest'
import { escapeStructuralLines, closeDanglingFence } from './syntax'

/** Parity suite with the Rust half (`backend/src/syntax.rs`'s tests for
 *  `escape_structural_lines` and `backend/src/convert.rs`'s
 *  `close_dangling_fence`). The two importers write the same files; a block
 *  the two sides escape differently can never be recognised as the same block
 *  by the sync merge, and the page doubles. */
describe('escapeStructuralLines', () => {
  it('escapes continuation lines that look like reserved properties', () => {
    expect(escapeStructuralLines('head\nid:: x')).toBe('head\n id:: x')
    // The first line IS the bullet's own text (it sits after `- `), never a property.
    expect(escapeStructuralLines('id:: x')).toBe('id:: x')
  })

  it('covers all nine keys the host PROP_RE recognises', () => {
    for (const key of ['type', 'line', 'id', 'collapsed', 'created', 'updated', 'status', 'answered', 'by']) {
      expect(escapeStructuralLines(`head\n${key}:: v`)).toBe(`head\n ${key}:: v`)
    }
  })

  it('leaves prop-shaped lines that PROP_RE would not match alone', () => {
    // No space after `::`, and a key outside the reserved set.
    expect(escapeStructuralLines('head\nid::x\nfoo:: bar')).toBe('head\nid::x\nfoo:: bar')
  })

  it('escapes continuation lines shaped like an outline bullet', () => {
    // A Roam shift-enter list. Left alone, `- milk` re-parses as a CHILD of the
    // block, which then loses its own `id::` and is re-created by every merge.
    expect(escapeStructuralLines('shopping\n- milk\n- eggs')).toBe('shopping\n - milk\n - eggs')
    // Nested items match the same pattern at any even indent.
    expect(escapeStructuralLines('a\n  - b')).toBe('a\n   - b')
    // The first line is the bullet's own text.
    expect(escapeStructuralLines('- milk')).toBe('- milk')
    // A dash that is not a bullet (odd indent, no trailing space, mid-line) is
    // already unambiguous and must not be touched.
    expect(escapeStructuralLines('a\n - b\n-dash\nx - y')).toBe('a\n - b\n-dash\nx - y')
  })

  it('leaves lines inside the fence the block itself opened exactly as they are', () => {
    // parseOutline takes those lines verbatim (raw mode), so there is nothing
    // to neutralize — and a space slipped into a YAML sample is the user's code,
    // silently altered.
    expect(escapeStructuralLines('```yaml\n- foo\n- bar\n```')).toBe('```yaml\n- foo\n- bar\n```')
    expect(escapeStructuralLines('```\nid:: not-a-property\n```')).toBe('```\nid:: not-a-property\n```')
    // Only a closer at least as long ends raw mode.
    expect(escapeStructuralLines('````\n```\n- inner\n````')).toBe('````\n```\n- inner\n````')
    // An unterminated fence runs to the end of the block.
    expect(escapeStructuralLines('```js\n- not a bullet')).toBe('```js\n- not a bullet')
  })

  it('escapes again once the fence closes', () => {
    expect(escapeStructuralLines('```\n- inside\n```\n- after\nid:: x')).toBe(
      '```\n- inside\n```\n - after\n id:: x',
    )
  })

  it('does not suspend the escape for a fence opened mid-block', () => {
    // parseOutline only enters raw mode from a bullet's FIRST line, so these
    // lines are still read as structure and must still be escaped.
    expect(escapeStructuralLines('prose\n```yaml\n- foo\n```')).toBe('prose\n```yaml\n - foo\n```')
  })

  it('is idempotent', () => {
    for (const s of ['head\nid:: x\n- milk', '```yaml\n- foo\n```\n- after', 'prose\n```\n- foo\n```']) {
      const once = escapeStructuralLines(s)
      expect(escapeStructuralLines(once)).toBe(once)
    }
    expect(escapeStructuralLines('head\nid:: x\n- milk')).toBe('head\n id:: x\n - milk')
  })
})

describe('closeDanglingFence', () => {
  it('appends the closer a block never wrote', () => {
    // Without it, raw mode runs past the end of this block and swallows the
    // blocks that follow on the next read.
    expect(closeDanglingFence('```js\nconst x = 1')).toBe('```js\nconst x = 1\n```')
    // The closer matches the opener's length.
    expect(closeDanglingFence('````\n```\ninner')).toBe('````\n```\ninner\n````')
  })

  it('leaves a block that closed its own fence alone', () => {
    expect(closeDanglingFence('```js\nconst x = 1\n```')).toBe('```js\nconst x = 1\n```')
    expect(closeDanglingFence('````\n```\ninner\n````')).toBe('````\n```\ninner\n````')
  })

  it('leaves a block that opens no fence alone', () => {
    expect(closeDanglingFence('prose\n```yaml\n- foo')).toBe('prose\n```yaml\n- foo')
    expect(closeDanglingFence('`inline`')).toBe('`inline`')
    expect(closeDanglingFence('')).toBe('')
  })

  it('is idempotent', () => {
    for (const s of ['```js\nconst x = 1', '````\n```\ninner', 'plain']) {
      const once = closeDanglingFence(s)
      expect(closeDanglingFence(once)).toBe(once)
    }
  })
})
