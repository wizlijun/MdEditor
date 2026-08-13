import { describe, it, expect } from 'vitest'
import { parseMarkdown } from '@moraya/core'
import { AllSelection } from 'prosemirror-state'
import { selectAllSelection } from './editor-select-all'

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
