import { describe, it, expect } from 'vitest'
import { newBaseTemplate } from './create'
import { parseBase } from './parse'
// Raw source of the two sides that must agree on the menu id (see below).
import libRs from '../../../src-tauri/src/lib.rs?raw'
import appSvelte from '../../App.svelte?raw'

describe('newBaseTemplate', () => {
  it('produces a valid single-table .base parseable with no error', () => {
    const cfg = parseBase(newBaseTemplate())
    expect(cfg.error).toBeUndefined()
    expect(cfg.views).toHaveLength(1)
    expect(cfg.views[0].type).toBe('table')
    expect(cfg.views[0].order).toContain('file.name')
  })
})

/**
 * `createNewBase` is reachable ONLY through the native File ▸ New Base item, so
 * the feature lives or dies by two strings agreeing across the Rust/TS boundary:
 * the menu id the Rust builder registers, and the `case` the frontend's
 * menu-event switch handles. Nothing else fails when they drift — the item just
 * silently stops doing anything (or disappears), which is exactly how the item
 * was lost once already when it rode in on the retired `base` plugin manifest.
 *
 * There is no seam to unit-test here (the switch is inline in App.svelte and the
 * menu builder needs a live Tauri AppHandle), so this asserts on the sources.
 */
describe('File ▸ New Base menu wiring', () => {
  const MENU_ID = 'new-base'

  it('is registered by the Rust menu builder', () => {
    expect(libRs).toContain(`MenuItemBuilder::with_id("${MENU_ID}"`)
    // …with a localized label rather than a hardcoded string.
    expect(libRs).toContain(`with_id("${MENU_ID}", menu_label(locale, "file.newBase"))`)
  })

  it('is handled by the frontend menu-event switch', () => {
    const handler = appSvelte
      .split('\n')
      .find((l) => l.includes(`case '${MENU_ID}':`))
    expect(handler, `no 'case ${MENU_ID}' in App.svelte's menu-event switch`).toBeDefined()
    expect(handler).toContain('createNewBase()')
  })
})
