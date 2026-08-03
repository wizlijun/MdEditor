import { describe, it, expect } from 'vitest'
import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

/**
 * Packaging-shape guard.
 *
 * `is_process_plugin()` (src-tauri/src/plugin_runtime/commands.rs) gives a
 * plugin a process and a lifecycle iff its manifest declares a non-empty
 * `binary` map. A `universal.notemdpkg` is by construction ui-only — it
 * carries no `bin/` — so publishing a binary-declaring manifest as `universal`
 * installs a process plugin with no process. That is exactly what roam-import
 * would have shipped at 1.1.0: the manifest grew a backend on 2026-08-03 while
 * `release_roam_import()` still built only the Vite bundle.
 *
 * The failure is silent at package time and only shows up on a user's machine,
 * so it is pinned statically here (and, at release time, by the guard inside
 * `zip_pkg`).
 */
const ROOT = join(__dirname, '..')
const SCRIPT = readFileSync(join(ROOT, 'scripts/release-plugins.sh'), 'utf8')

/** `plugin-arg → release_fn` from the dispatch `case` at the foot of the script. */
function dispatchTable(): Map<string, string> {
  const table = new Map<string, string>()
  for (const m of SCRIPT.matchAll(/^\s{4}([a-z0-9-]+)\)\s+(release_[a-z0-9_]+)\s*;;/gm)) {
    table.set(m[1], m[2])
  }
  return table
}

/** A shell function's body, by brace depth (the bodies here nest one level). */
function functionBody(name: string): string {
  const start = SCRIPT.indexOf(`${name}() {`)
  expect(start, `${name}() is not defined in release-plugins.sh`).toBeGreaterThan(-1)
  let depth = 0
  for (let i = SCRIPT.indexOf('{', start); i < SCRIPT.length; i++) {
    if (SCRIPT[i] === '{') depth++
    else if (SCRIPT[i] === '}' && --depth === 0) return SCRIPT.slice(start, i + 1)
  }
  throw new Error(`unbalanced braces in ${name}()`)
}

/**
 * The body that actually zips, following the one level of delegation the
 * script uses (`release_roam_import` → `release_native_ui`, etc.).
 */
function packagingBody(fn: string): string {
  const body = functionBody(fn)
  const delegate = body.match(/\brelease_(native_ui|native_bin)\b/)
  return delegate ? functionBody(`release_${delegate[1]}`) : body
}

const plugins = readdirSync(join(ROOT, 'plugins-src'), { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(ROOT, 'plugins-src', d.name, 'manifest.v2.json')))
  .map((d) => ({
    dir: d.name,
    manifest: JSON.parse(
      readFileSync(join(ROOT, 'plugins-src', d.name, 'manifest.v2.json'), 'utf8'),
    ) as { id: string; binary?: Record<string, string> },
  }))

describe('release-plugins.sh packaging shape', () => {
  const table = dispatchTable()

  it('dispatches every plugin it accepts as an argument', () => {
    expect(table.size).toBeGreaterThan(0)
    for (const [arg, fn] of table) expect(functionBody(fn), arg).toBeTruthy()
  })

  it.each(plugins.filter((p) => Object.keys(p.manifest.binary ?? {}).length > 0))(
    '$dir declares a binary, so it must be packaged per triple',
    ({ dir, manifest }) => {
      const fn = table.get(dir)
      expect(fn, `${dir} declares ${Object.keys(manifest.binary!).length} binary target(s) but release-plugins.sh has no case for it`).toBeDefined()
      const body = packagingBody(fn!)
      expect(body, `${dir} is packaged as universal.notemdpkg`).not.toContain('universal.notemdpkg')
      expect(body, `${dir} emits no per-triple package`).toContain('$triple.notemdpkg')
    },
  )

  it.each(plugins.filter((p) => Object.keys(p.manifest.binary ?? {}).length === 0))(
    '$dir declares no binary, so universal is the right shape',
    ({ dir }) => {
      const fn = table.get(dir)
      // Fixtures (custom-editor-fixture) are never released; only assert the
      // shape of the ones the script knows about.
      if (!fn) return
      expect(packagingBody(fn)).toContain('universal.notemdpkg')
    },
  )

  it('refuses a universal package whose manifest declares a binary', () => {
    // The runtime half of the same rule: even a hand-written release function
    // cannot get a binary-declaring manifest into a universal package.
    const zip = functionBody('zip_pkg')
    expect(zip).toContain('universal.notemdpkg')
    expect(zip).toContain('manifest_binary_count')
    expect(zip).toMatch(/exit 4/)
  })
})
