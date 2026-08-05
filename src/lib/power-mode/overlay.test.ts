/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, afterEach } from 'vitest'
import { acquireOverlay, releaseOverlay } from './overlay'

describe('overlay', () => {
  afterEach(() => { document.body.innerHTML = '' })

  it('creates one node and hands the same one to every caller', () => {
    const a = acquireOverlay()
    const b = acquireOverlay()
    expect(a).toBe(b)
    expect(document.querySelectorAll('.power-mode-overlay')).toHaveLength(1)
    releaseOverlay(); releaseOverlay()
  })

  it('removes the node only when the last holder releases it', () => {
    acquireOverlay(); acquireOverlay()
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).not.toBeNull()
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).toBeNull()
  })

  it('ignores a release with no outstanding acquire', () => {
    releaseOverlay()
    expect(document.querySelector('.power-mode-overlay')).toBeNull()
    // 之后仍然能正常创建
    acquireOverlay()
    expect(document.querySelector('.power-mode-overlay')).not.toBeNull()
    releaseOverlay()
  })
})
