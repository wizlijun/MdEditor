import { describe, it, expect, vi, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { EditorState, AllSelection, TextSelection, type Transaction } from 'prosemirror-state'
import { parseMarkdown } from '@moraya/core'
import {
  selectAllSelection, isSelectAllShortcut, applySelectAll, handleSelectAllKeydown,
} from './editor-select-all'

let isApple = true
vi.mock('./platform-sync', () => ({ isApplePlatformSync: () => isApple }))
beforeEach(() => { isApple = true })

const WITH_FM = `---
title: t
tags: [a, b]
---

# Heading

body text
`

const PLAIN = `# Heading

body text
`

const WITH_CODE = `# Heading

\`\`\`js
const a = 1
\`\`\`

trailing paragraph
`

describe('selectAllSelection', () => {
  it('covers the whole doc when there is no frontmatter', () => {
    const doc = parseMarkdown(PLAIN)
    const sel = selectAllSelection(doc)
    expect(sel).toBeInstanceOf(AllSelection)
    expect(sel.from).toBe(0)
    expect(sel.to).toBe(doc.content.size)
  })

  it('starts after the frontmatter node so the range stays inside editable content', () => {
    const doc = parseMarkdown(WITH_FM)
    const fm = doc.firstChild!
    // Guard the premise: the fixture really does parse into a frontmatter node.
    expect(fm.type.name).toBe('frontmatter')

    const sel = selectAllSelection(doc)
    expect(sel).not.toBeInstanceOf(AllSelection)
    // Anything at or before fm.nodeSize - 1 would put the DOM range's start
    // inside the contenteditable=false NodeView, which is what clamps the
    // painted selection to the metadata block.
    expect(sel.from).toBeGreaterThanOrEqual(fm.nodeSize)
    expect(sel.to).toBe(doc.content.size - 1)
    expect(doc.textBetween(sel.from, sel.to, ' ')).toContain('body text')
    expect(doc.textBetween(sel.from, sel.to, ' ')).not.toContain('title')
  })

  it('falls back to AllSelection when frontmatter is all there is', () => {
    const doc = parseMarkdown('---\ntitle: t\n---\n')
    expect(doc.firstChild!.type.name).toBe('frontmatter')
    expect(selectAllSelection(doc)).toBeInstanceOf(AllSelection)
  })
})

// ── The Cmd+A path ──────────────────────────────────────────────────────────
// Until this was wired up, Cmd+A never reached `selectAllSelection`: the chord
// was registered as a native Edit-menu accelerator, and whatever slipped past
// it landed on moraya-core's own `Mod-a` binding, which applies a raw
// AllSelection — or, with the caret inside a code block, selects *only that
// block*. Both read to the user as "Cmd+A only selects part of the document",
// while the right-click menu (which does call selectAllSelection) was fine.

/** Minimal stand-in for EditorView: mounting real moraya needs a DOM it can't get here. */
function stubView(markdown: string, caretAt?: (state: EditorState) => number) {
  let state = EditorState.create({ doc: parseMarkdown(markdown) })
  if (caretAt) state = state.apply(state.tr.setSelection(TextSelection.create(state.doc, caretAt(state))))
  let focused = 0
  return {
    get state() { return state },
    dispatch: (tr: Transaction) => { state = state.apply(tr) },
    focus: () => { focused++ },
    get focusCount() { return focused },
  }
}

/** Position of the first character inside the doc's only code_block. */
function insideCodeBlock(state: EditorState): number {
  let pos = -1
  state.doc.descendants((node, at) => {
    if (pos < 0 && node.type.name === 'code_block') pos = at + 1
  })
  if (pos < 0) throw new Error('fixture has no code_block')
  return pos
}

function keyEvent(init: Partial<KeyboardEvent> & { key: string }) {
  return {
    key: init.key,
    metaKey: init.metaKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  }
}

describe('isSelectAllShortcut', () => {
  it('matches Cmd+A on Apple platforms', () => {
    expect(isSelectAllShortcut(keyEvent({ key: 'a', metaKey: true }))).toBe(true)
    expect(isSelectAllShortcut(keyEvent({ key: 'A', metaKey: true }))).toBe(true)
  })

  it('leaves Ctrl+A alone on Apple platforms — that is move-to-line-start there', () => {
    expect(isSelectAllShortcut(keyEvent({ key: 'a', ctrlKey: true }))).toBe(false)
  })

  it('matches Ctrl+A, not Cmd+A, off Apple platforms', () => {
    isApple = false
    expect(isSelectAllShortcut(keyEvent({ key: 'a', ctrlKey: true }))).toBe(true)
    expect(isSelectAllShortcut(keyEvent({ key: 'a', metaKey: true }))).toBe(false)
  })

  it('does not match when Shift or Alt is held, or for other keys', () => {
    expect(isSelectAllShortcut(keyEvent({ key: 'a', metaKey: true, shiftKey: true }))).toBe(false)
    expect(isSelectAllShortcut(keyEvent({ key: 'a', metaKey: true, altKey: true }))).toBe(false)
    expect(isSelectAllShortcut(keyEvent({ key: 'b', metaKey: true }))).toBe(false)
    expect(isSelectAllShortcut(keyEvent({ key: 'a' }))).toBe(false)
  })
})

describe('applySelectAll', () => {
  it('selects the whole document even when the caret sits inside a code block', () => {
    const view = stubView(WITH_CODE, insideCodeBlock)
    // Premise: the caret really is inside the code block, which is the case
    // moraya-core's Mod-a narrows the selection to.
    expect(view.state.selection.$from.parent.type.name).toBe('code_block')

    applySelectAll(view)

    expect(view.state.selection.from).toBe(0)
    expect(view.state.selection.to).toBe(view.state.doc.content.size)
    expect(view.state.doc.textBetween(view.state.selection.from, view.state.selection.to, '\n'))
      .toContain('trailing paragraph')
  })

  it('applies the frontmatter-aware range, same as the right-click menu', () => {
    const view = stubView(WITH_FM)
    applySelectAll(view)
    expect(view.state.selection).not.toBeInstanceOf(AllSelection)
    expect(view.state.selection.from).toBeGreaterThanOrEqual(view.state.doc.firstChild!.nodeSize)
  })

  it('focuses the view so the range actually gets painted', () => {
    const view = stubView(PLAIN)
    applySelectAll(view)
    expect(view.focusCount).toBeGreaterThan(0)
  })
})

describe('handleSelectAllKeydown', () => {
  it('consumes the chord and stops it reaching moraya-core\'s Mod-a binding', () => {
    const view = stubView(WITH_CODE, insideCodeBlock)
    const ev = keyEvent({ key: 'a', metaKey: true })

    expect(handleSelectAllKeydown(ev, view)).toBe(true)
    expect(ev.preventDefault).toHaveBeenCalled()
    // Without stopPropagation, moraya's `Mod-a` (bound on the ProseMirror
    // element) re-narrows the selection back to the code block.
    expect(ev.stopPropagation).toHaveBeenCalled()
    expect(view.state.selection.to).toBe(view.state.doc.content.size)
  })

  it('ignores everything else and leaves the selection untouched', () => {
    const view = stubView(PLAIN)
    const before = view.state.selection
    const ev = keyEvent({ key: 'b', metaKey: true })

    expect(handleSelectAllKeydown(ev, view)).toBe(false)
    expect(ev.preventDefault).not.toHaveBeenCalled()
    expect(view.state.selection).toBe(before)
    expect(view.focusCount).toBe(0)
  })
})

describe('the Cmd+A delivery path', () => {
  it('does not register Cmd+A as a native menu accelerator', () => {
    // A menu key-equivalent is swallowed by macOS in `performKeyEquivalent:`,
    // so the webview never sees the keydown and select-all has to survive a
    // menu focus round-trip — during which WebKit restores its own cached DOM
    // selection over ours. The item stays (menu-click still broadcasts
    // `notemd:select-all`); only the accelerator goes, so the chord reaches
    // the editor the same way the right-click menu does.
    const rust = readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8')
    const item = rust.split('\n').find((l) => l.includes('with_id("select-all"'))
    expect(item, 'select-all menu item should still exist').toBeTruthy()
    expect(item).not.toMatch(/\.accelerator\(/)
  })
})
