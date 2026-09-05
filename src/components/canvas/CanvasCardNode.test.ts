// @vitest-environment node
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const source = readFileSync(fileURLToPath(new URL('./CanvasCardNode.svelte', import.meta.url)), 'utf8')

describe('CanvasCardNode interaction contracts', () => {
  it('lets Canvas own pointer gestures over an inactive image body', () => {
    expect(source).not.toMatch(/class="file-image[^\"]*\bnodrag\b/)
    expect(source).not.toMatch(/class="file-image[^\"]*\bnopan\b/)
    expect(source).toContain('draggable="false"')
  })

  it('remounts an active editor when its path or resolver root changes', () => {
    expect(source).toContain('let editorMountKey = $derived')
    expect(source).toContain('data.canvasPath')
    expect(source).toContain('mediaResolverRoot(data.mediaResolver)')
    expect(source).toContain('{#key editorMountKey}')
  })

  it('provides 44px coarse-pointer hit targets for connection and resize handles', () => {
    expect(source).toContain('handleClass="canvas-card-resize-handle"')
    expect(source).toMatch(/@media \(pointer: coarse\)[\s\S]*:global\(\.canvas-handle\)[\s\S]*width: 44px;[\s\S]*height: 44px;/)
    expect(source).toMatch(/@media \(pointer: coarse\)[\s\S]*:global\(\.canvas-card-resize-handle\)[\s\S]*width: 44px !important;[\s\S]*height: 44px !important;/)
  })
})
