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
const DEV_INSTALL = readFileSync(join(ROOT, 'scripts/dev-install-plugin.sh'), 'utf8')

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

  it('builds and packages codex-agent as a native plugin with UI', () => {
    expect(table.get('codex-agent')).toBe('release_codex_agent')
    const body = functionBody('release_codex_agent')
    expect(body).toContain('notemd.codex-agent')
    expect(body).toContain('plugins-src/codex-agent')
    expect(body).toContain('notemd-codex-agent')
    expect(body).toContain('"codex-agent"')
    expect(packagingBody('release_codex_agent')).toContain('$triple.notemdpkg')
    expect(packagingBody('release_codex_agent')).toContain('cargo build --release --locked')
  })
})

describe('dev-install-plugin.sh codex-agent dispatch', () => {
  it('accepts, builds and installs the codex-agent backend and UI', () => {
    expect(DEV_INSTALL).toMatch(/\|codex-agent\|/)
    expect(DEV_INSTALL).toContain('elif [[ "$PLUGIN" == "codex-agent" ]]')
    expect(DEV_INSTALL).toContain('--bin notemd-codex-agent')
    expect(DEV_INSTALL).toContain('pnpm --filter codex-agent build')
    expect(DEV_INSTALL).toContain('mark_installed "notemd.codex-agent" "$VERSION"')
    expect(DEV_INSTALL).toContain('"$DEST/bin/notemd-codex-agent"')
    expect(DEV_INSTALL).toContain('"$DEST/ui/"')
  })
})

describe('Next plugin packaging', () => {
  it('builds and packages Next as a universal UI plugin', () => {
    const table = dispatchTable()
    expect(table.get('next')).toBe('release_next')
    const release = functionBody('release_next')
    expect(release).toContain('notemd.next')
    expect(release).toContain('plugins-src/next')
    expect(release).toContain('pnpm --filter next-plugin build')
    expect(packagingBody('release_next')).toContain('universal.notemdpkg')
  })

  it('accepts, builds and installs Next in the dev installer', () => {
    expect(DEV_INSTALL).toMatch(/\|next\|/)
    expect(DEV_INSTALL).toContain('elif [[ "$PLUGIN" == "next" ]]')
    expect(DEV_INSTALL).toContain('pnpm --filter next-plugin build')
    expect(DEV_INSTALL).toContain('mark_installed "notemd.next" "$VERSION"')
    expect(DEV_INSTALL).toContain('"$DEST/ui/"')
  })

  it('keeps Next settings inside the plugin window instead of global Settings', () => {
    const manifest = JSON.parse(
      readFileSync(join(ROOT, 'plugins-src', 'next', 'manifest.v2.json'), 'utf8'),
    ) as {
      version?: string
      capabilities?: string[]
      contributes?: { settings?: unknown }
      i18n?: Record<string, Record<string, unknown>>
    }

    expect(manifest.version).toBe('1.6.3')
    expect(manifest.contributes?.settings).toBeUndefined()
    expect(manifest.capabilities).toContain('settings')
    for (const catalog of Object.values(manifest.i18n ?? {})) {
      expect(catalog['settings.tab_label']).toBeUndefined()
      expect(catalog['settings.fields']).toBeUndefined()
    }
  })
})

describe('agent-owned concurrency settings', () => {
  const agentDirs = ['claude-agent', 'codex-agent', 'deepseek-agent']

  it.each(agentDirs)('%s keeps concurrency out of the global Settings dialog', (dir) => {
    const manifest = JSON.parse(
      readFileSync(join(ROOT, 'plugins-src', dir, 'manifest.v2.json'), 'utf8'),
    ) as {
      capabilities?: string[]
      engines?: { notemd?: string }
      contributes?: { settings?: unknown }
      i18n?: Record<string, Record<string, unknown>>
    }

    expect(manifest.contributes?.settings).toBeUndefined()
    expect(manifest.capabilities).toContain('settings')
    expect(manifest.capabilities).not.toContain('agent')
    expect(manifest.engines?.notemd).toBe('>=6.828.1')
    for (const catalog of Object.values(manifest.i18n ?? {})) {
      expect(catalog['settings.tab_label']).toBeUndefined()
      expect(catalog['settings.fields']).toBeUndefined()
    }
  })
})

describe('plugin market metadata localization', () => {
  const publishedDirs = [
    'claude-agent',
    'codex-agent',
    'decision-log',
    'deepseek-agent',
    'ebook-import',
    'idea-spark',
    'md2pdf',
    'memory',
    'next',
    'openclaw',
    'pos-log',
    'power-mode',
    'roam-import',
    'trace-source',
    'weekly-review',
  ]

  it.each(publishedDirs)('%s provides a name and description for every market locale', (dir) => {
    const manifest = JSON.parse(
      readFileSync(join(ROOT, 'plugins-src', dir, 'manifest.v2.json'), 'utf8'),
    ) as {
      i18n?: Record<string, { name?: unknown; description?: unknown }>
    }

    for (const locale of ['zh', 'ja', 'de']) {
      expect(manifest.i18n?.[locale]?.name, `${dir}: missing ${locale} name`)
        .toEqual(expect.any(String))
      expect(manifest.i18n?.[locale]?.description, `${dir}: missing ${locale} description`)
        .toEqual(expect.any(String))
      expect((manifest.i18n?.[locale]?.description as string).trim()).not.toBe('')
    }
  })
})

describe('Location Log product identity', () => {
  const manifest = JSON.parse(
    readFileSync(join(ROOT, 'plugins-src', 'pos-log', 'manifest.v2.json'), 'utf8'),
  ) as {
    id: string
    name: string
    contributes: { menus: Array<{ command: string; label: string }> }
    i18n: Record<string, { name: string; menus: Record<string, string> }>
  }

  it('keeps the stable plugin id while using one display name per locale', () => {
    expect(manifest.id).toBe('notemd.pos-log')
    expect(manifest.name).toBe('Location Log')
    expect(manifest.contributes.menus).toContainEqual(
      expect.objectContaining({ command: 'save-now', label: 'Location Log' }),
    )

    expect(manifest.i18n.zh).toMatchObject({
      name: '位置记录',
      menus: { 'save-now': '位置记录' },
    })
    expect(manifest.i18n.ja).toMatchObject({
      name: '位置記録',
      menus: { 'save-now': '位置記録' },
    })
    expect(manifest.i18n.de).toMatchObject({
      name: 'Standortprotokoll',
      menus: { 'save-now': 'Standortprotokoll' },
    })
  })
})

describe('Next tray shortcut', () => {
  const manifest = JSON.parse(
    readFileSync(join(ROOT, 'plugins-src', 'next', 'manifest.v2.json'), 'utf8'),
  ) as {
    contributes: {
      tray?: Array<{ window: string; section?: string; accelerator?: string }>
    }
  }

  it('opens Next from the capture group with its global shortcut', () => {
    expect(manifest.contributes.tray).toContainEqual({
      window: 'main',
      section: 'capture',
      accelerator: 'Cmd+Ctrl+N',
    })
  })
})
