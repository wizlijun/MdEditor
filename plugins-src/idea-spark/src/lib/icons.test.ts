import { describe, expect, it } from 'vitest'
import { ICONS, type IconName } from './icons'

// The icon table is data, so the house rules that keep the set coherent can be
// checked as data rather than trusted to a reviewer's eye. They are not style
// preferences: an icon that carries its own `fill` turns into a solid blob at
// 12px, and one that carries its own color stops following `currentColor` —
// which is exactly what `failed` (warning color) and `delete` (danger color)
// depend on, and exactly what the emoji this set replaced could not do.

/** The key set the table is supposed to have — nothing more. Renaming a key
 *  and updating its call sites is TypeScript's problem; what this pins is that
 *  the set itself doesn't quietly grow or shrink. */
const EXPECTED: IconName[] = [
  'new-idea',
  'delegate',
  'inbox',
  'settings',
  'running',
  'failed',
  'open-idea',
  'open-proof',
  'rename',
  'delete',
]

const entries = Object.entries(ICONS) as Array<[IconName, string]>

describe('the icon table', () => {
  it('holds exactly the icons the window draws', () => {
    expect(Object.keys(ICONS).sort()).toEqual([...EXPECTED].sort())
  })

  it.each(entries)('%s is stroke-only — no fill of its own', (_name, body) => {
    // `fill` belongs to the shared wrapper (`fill="none"`); a shape that opts
    // back into filling reads as a black lump next to the others.
    expect(body).not.toMatch(/\bfill\s*=/)
  })

  it.each(entries)('%s carries no color of its own', (_name, body) => {
    // No `stroke=`, no hex, no rgb()/hsl(), no `opacity`. The wrapper's
    // `stroke="currentColor"` must be the only thing deciding how it looks.
    expect(body).not.toMatch(/\bstroke\s*=/)
    expect(body).not.toMatch(/#[0-9a-fA-F]{3}/)
    expect(body).not.toMatch(/\b(?:rgb|hsl)a?\(/)
    expect(body).not.toMatch(/\bopacity\s*=/)
    // Both of the back doors around the four checks above: a presentation
    // attribute can be restated in `style="stroke:red"` (which wins over the
    // inherited one), and a `class` can pull in a rule from anywhere in the
    // bundle. Neither belongs in a table whose entire job is to defer to
    // `currentColor`.
    expect(body).not.toMatch(/\bstyle\s*=/)
    expect(body).not.toMatch(/\bclass\s*=/)
  })

  it.each(entries)('%s uses only primitive shapes', (_name, body) => {
    const tags = [...body.matchAll(/<([a-zA-Z][\w-]*)/g)].map((m) => m[1])
    expect(tags.length).toBeGreaterThan(0)
    // No <image>/<filter>/<mask>/<style>/<text>: they either fail to inherit
    // the current color, need an external resource, or put glyphs in an icon.
    for (const tag of tags) {
      expect(['path', 'circle', 'line', 'polyline', 'rect']).toContain(tag)
    }
  })

  it.each(entries)('%s is drawn with few enough strokes to read at 12px', (_name, body) => {
    // The design constraint from the spec: at most 4 separate shapes. Below
    // that limit an icon survives being shrunk to a 12px status badge; above
    // it, it smears.
    const shapes = [...body.matchAll(/<[a-zA-Z]/g)].length
    expect(shapes).toBeGreaterThanOrEqual(1)
    expect(shapes).toBeLessThanOrEqual(4)
  })

  it.each(entries)('%s is well-formed self-closing markup', (_name, body) => {
    // Every shape closes itself (`<path …/>`) and nothing wraps anything: the
    // body is injected with `{@html}`, so an unbalanced tag would silently eat
    // whatever the browser decided to repair it with.
    expect(body).toMatch(/^(?:<[a-zA-Z][^<>]*\/>)+$/)
  })
})
