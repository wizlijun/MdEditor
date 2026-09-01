import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const source = readFileSync(new URL('./plugin-market-app.svelte', import.meta.url), 'utf8')
const style = source.match(/<style>([\s\S]*?)<\/style>/)?.[1] ?? ''

function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return style.match(new RegExp(`${escaped}\\s*\\{([^}]+)\\}`))?.[1] ?? ''
}

describe('plugin market standard window colors', () => {
  it('keeps window, spotlight, category, and card surfaces neutral', () => {
    expect(rule('main')).toContain('background: var(--window-background)')
    expect(rule('.ai-spotlight')).toContain('background: var(--window-surface)')
    expect(rule('.category-block')).toContain('background: var(--window-surface)')
    expect(rule('.plugin-card')).toContain('background: var(--card-surface)')
    expect(rule('.ai-card')).not.toContain('gradient')
    expect(rule('.update-card')).not.toMatch(/#f08a00|#e97700|gradient/)
  })

  it('reserves category color for icon surfaces', () => {
    expect(rule('.category-mark')).toContain('background: var(--accent)')
    expect(rule('.plugin-mark')).toContain('background: var(--accent-soft)')
    expect(rule('.system-badge')).not.toContain('var(--accent)')
    expect(rule('.status.enabled,\n  .available-status')).not.toContain('var(--accent)')
  })
})
