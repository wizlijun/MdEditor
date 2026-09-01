#!/usr/bin/env node
// Build the marketplace registry index from packaged plugins (子项目③ Task 5).
//
//   node scripts/gen-plugin-index.mjs [--local-only] [--drop <id[@version]>]...
//
// Scans dist-plugins/<id>/<version>/ produced by scripts/release-plugins.sh:
//   - reads the manifest.json that release-plugins.sh drops next to the packages
//   - finds every <arch>.notemdpkg sibling (arch = the triple, or "universal"
//     for ui-only plugins)
//   - computes each package's size + sha256
//   - emits one RegistryEntry per <id>/<version>
//
// It then MERGES those entries with the live index (GET /api/index.json):
// dist-plugins/ is gitignored and per-worktree, so it only ever holds the
// plugins built HERE — publishing it verbatim once wiped live plugins from
// the market (they had been packaged in a different worktree).
// The union keeps every live entry we did not rebuild; a local entry replaces
// the live one with the same id@version.
//
//   --local-only          skip the live fetch (first publish / registry down —
//                         an explicit choice, never a silent fallback)
//   --drop id[@version]   remove an entry (or every version of an id) from the
//                         merged output — the only way to unpublish, since the
//                         merge otherwise preserves live entries forever
//
// The output shape mirrors src-tauri/src/plugin_runtime/market.rs RegistryEntry
// and is what the CF Worker serves at GET /api/index.json.
//
// Does NOT upload. Tail prints the wrangler kv command as guidance.

import { readFileSync, readdirSync, writeFileSync, statSync, existsSync, mkdirSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { join, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const OUT_ROOT = join(REPO_ROOT, 'dist-plugins')
const REGISTRY_BASE = 'https://plugins.notemd.net'
const PLUGIN_CATEGORIES = new Set([
  'record', 'reading', 'inspiration', 'advance', 'reflect', 'create', 'import-export', 'experience',
])
const LEGACY_PLUGIN_CATEGORIES = new Map([
  ['agents', 'advance'],
  ['capture', 'record'],
  ['capture-import', 'record'],
  ['thinking', 'reflect'],
  ['thinking-review', 'reflect'],
  ['publish-export', 'import-export'],
  ['editing', 'experience'],
  ['editor-extensions', 'experience'],
])
const OFFICIAL_PLUGIN_CATEGORIES = new Map([
  ['notemd.pos-log', 'record'],
  ['notemd.roam-import', 'import-export'],
  ['notemd.ebook-import', 'reading'],
  ['notemd.trace-source', 'reading'],
  ['notemd.idea-spark', 'inspiration'],
  ['notemd.next', 'advance'],
  ['notemd.claude-agent', 'advance'],
  ['notemd.codex-agent', 'advance'],
  ['notemd.deepseek-agent', 'advance'],
  ['notemd.openclaw-chat', 'advance'],
  ['notemd.decision-log', 'reflect'],
  ['notemd.weekly-review', 'reflect'],
  ['notemd.md2pdf', 'import-export'],
  ['notemd.power-mode', 'experience'],
  ['notemd.memory', 'reflect'],
])

/**
 * The primary marketplace category reuses the first menu contribution's
 * one-level submenu key. The same manifest field drives the native Plugins
 * menu, so membership cannot drift between surfaces. Unknown/missing values
 * remain visible under Other.
 */
export function pluginCategoryFromManifest(manifest) {
  const official = OFFICIAL_PLUGIN_CATEGORIES.get(manifest?.id)
  if (official) return official
  const menus = manifest?.contributes?.menus
  const raw = Array.isArray(menus)
    ? menus.find((menu) => typeof menu?.submenu === 'string')?.submenu
    : null
  if (PLUGIN_CATEGORIES.has(raw)) return raw
  return LEGACY_PLUGIN_CATEGORIES.get(raw) ?? 'other'
}

/**
 * Refresh user-facing registry copy from source manifests without rebuilding or
 * replacing immutable plugin packages. Package/version/hash/download fields
 * remain untouched; unknown third-party entries pass through verbatim.
 */
export function applySourceMetadata(entries, manifests) {
  const byId = new Map(manifests.map((manifest) => [manifest.id, manifest]))
  return entries.map((entry) => {
    const manifest = byId.get(entry.id)
    if (!manifest) return entry
    return {
      ...entry,
      name: manifest.name ?? entry.name,
      category: pluginCategoryFromManifest(manifest),
      description: manifest.description ?? entry.description ?? null,
      i18n: manifest.i18n ?? entry.i18n ?? null,
    }
  })
}

function sha256Hex(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function dirsIn(path) {
  if (!existsSync(path)) return []
  return readdirSync(path, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
}

function pkgsIn(path) {
  return readdirSync(path, { withFileTypes: true })
    .filter((d) => d.isFile() && d.name.endsWith('.notemdpkg'))
    .map((d) => d.name)
}

function buildEntry(idDirName, version, versionDir) {
  const manifestPath = join(versionDir, 'manifest.json')
  if (!existsSync(manifestPath)) {
    throw new Error(`missing manifest.json in ${versionDir} — run scripts/release-plugins.sh first`)
  }
  const m = JSON.parse(readFileSync(manifestPath, 'utf8'))
  const id = m.id ?? idDirName

  // arch = the .notemdpkg basename without extension: a triple, or "universal".
  const archs = []
  const sha256 = {}
  const download = {}
  const sizes = []
  for (const file of pkgsIn(versionDir).sort()) {
    const arch = file.replace(/\.notemdpkg$/, '')
    const pkgPath = join(versionDir, file)
    archs.push(arch)
    sha256[arch] = sha256Hex(pkgPath)
    sizes.push(statSync(pkgPath).size)
    download[arch] = `${REGISTRY_BASE}/api/download/${id}/${version}/${arch}`
  }
  if (archs.length === 0) {
    throw new Error(`no .notemdpkg found in ${versionDir}`)
  }

  return {
    id,
    version: m.version ?? version,
    // min_host = the engines.notemd semver range verbatim (e.g. ">=6.716.7").
    min_host: m.engines?.notemd ?? '>=0.0.0',
    archs,
    // size = the largest package (single number in RegistryEntry); per-arch
    // exactness lives in sha256/download maps.
    size: Math.max(...sizes),
    sha256,
    name: m.name ?? id,
    category: pluginCategoryFromManifest(m),
    description: m.description ?? null,
    i18n: m.i18n ?? null,
    icon_url: m.icon_url ?? null,
    changelog_url: m.changelog_url ?? null,
    download,
  }
}

/** Numeric dotted-component compare (1.0.9 < 1.0.10); missing components = 0. */
export function compareVersions(a, b) {
  const pa = a.split('.').map((p) => parseInt(p, 10) || 0)
  const pb = b.split('.').map((p) => parseInt(p, 10) || 0)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = pa[i] ?? 0
    const y = pb[i] ?? 0
    if (x !== y) return x - y
  }
  return 0
}

/**
 * Union local (freshly built) and live (currently published) entries by
 * id@version. Local wins a key collision — the operator just rebuilt it; live
 * entries with no local counterpart survive untouched, so a partial worktree
 * can never unpublish someone else's plugin. `drops` ("id" or "id@version")
 * removes entries from either side.
 *
 * Returns { plugins, added, replaced, unchanged, kept, dropped } — the four
 * lists are id@version keys for the publish summary.
 */
export function mergeIndexes(localPlugins, livePlugins, drops = []) {
  const dropIds = new Set(drops.filter((d) => !d.includes('@')))
  const dropKeys = new Set(drops.filter((d) => d.includes('@')))
  const isDropped = (e) => dropIds.has(e.id) || dropKeys.has(`${e.id}@${e.version}`)
  const key = (e) => `${e.id}@${e.version}`

  const merged = new Map()
  const dropped = new Set()
  const added = []
  const replaced = []
  const unchanged = []
  const kept = []

  const liveByKey = new Map()
  for (const e of livePlugins) {
    if (isDropped(e)) {
      dropped.add(key(e))
      continue
    }
    liveByKey.set(key(e), e)
    merged.set(key(e), e)
  }

  for (const e of localPlugins) {
    if (isDropped(e)) {
      dropped.add(key(e))
      continue
    }
    const k = key(e)
    const live = liveByKey.get(k)
    if (!live) added.push(k)
    else if (JSON.stringify(live) === JSON.stringify(e)) unchanged.push(k)
    else replaced.push(k)
    merged.set(k, e)
  }

  for (const k of liveByKey.keys()) {
    if (!added.includes(k) && !replaced.includes(k) && !unchanged.includes(k)) kept.push(k)
  }

  const plugins = [...merged.values()].sort(
    (a, b) => a.id.localeCompare(b.id) || compareVersions(a.version, b.version),
  )
  return {
    plugins,
    added: added.sort(),
    replaced: replaced.sort(),
    unchanged: unchanged.sort(),
    kept: kept.sort(),
    dropped: [...dropped].sort(),
  }
}

function scanLocal() {
  const plugins = []
  for (const idDirName of dirsIn(OUT_ROOT).sort()) {
    const idDir = join(OUT_ROOT, idDirName)
    for (const version of dirsIn(idDir).sort()) {
      const versionDir = join(idDir, version)
      // Skip dirs with no packages (e.g. a stale manifest-only leftover).
      if (pkgsIn(versionDir).length === 0) continue
      plugins.push(buildEntry(idDirName, version, versionDir))
    }
  }
  return plugins
}

function scanSourceManifests() {
  const sourceRoot = join(REPO_ROOT, 'plugins-src')
  const manifests = []
  for (const dirName of dirsIn(sourceRoot).sort()) {
    const path = join(sourceRoot, dirName, 'manifest.v2.json')
    if (!existsSync(path)) continue
    manifests.push(JSON.parse(readFileSync(path, 'utf8')))
  }
  return manifests
}

async function fetchLive() {
  const url = `${REGISTRY_BASE}/api/index.json`
  const resp = await fetch(url)
  if (!resp.ok) throw new Error(`live index fetch failed: ${resp.status} for ${url}`)
  const index = await resp.json()
  if (!Array.isArray(index?.plugins)) throw new Error(`live index has no plugins[] (${url})`)
  return index.plugins
}

async function main() {
  const args = process.argv.slice(2)
  const drops = []
  let localOnly = false
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--local-only') localOnly = true
    else if (args[i] === '--drop' && args[i + 1]) drops.push(args[++i])
    else {
      console.error(`unknown arg: ${args[i]} (expected --local-only | --drop <id[@version]>)`)
      process.exit(2)
    }
  }

  if (!existsSync(OUT_ROOT) && localOnly) {
    console.error(`no dist-plugins/ — run scripts/release-plugins.sh first`)
    process.exit(1)
  }

  const local = scanLocal()

  let live = []
  if (localOnly) {
    console.log('⚠ --local-only: skipping the live index — the upload will REPLACE it wholesale')
  } else {
    // A fetch failure aborts rather than degrading to local-only: uploading a
    // local-only index by accident is exactly the clobber this merge prevents.
    live = await fetchLive()
  }

  const r = mergeIndexes(local, live, drops)
  const plugins = applySourceMetadata(r.plugins, scanSourceManifests())
  mkdirSync(OUT_ROOT, { recursive: true })
  const outPath = join(OUT_ROOT, 'index.json')
  writeFileSync(outPath, JSON.stringify({ plugins }, null, 2) + '\n')

  console.log(`wrote ${outPath} (${plugins.length} plugin version(s))`)
  if (r.added.length) console.log(`  added     (new here):        ${r.added.join(', ')}`)
  if (r.replaced.length) console.log(`  replaced  (local ≠ live):    ${r.replaced.join(', ')}`)
  if (r.unchanged.length) console.log(`  unchanged (local == live):   ${r.unchanged.join(', ')}`)
  if (r.kept.length) console.log(`  kept      (live only):       ${r.kept.join(', ')}`)
  if (r.dropped.length) console.log(`  dropped   (--drop):          ${r.dropped.join(', ')}`)
  if (r.replaced.length) {
    console.log('⚠ replaced entries change sha256/metadata for an already-published version:')
    console.log('  re-upload those packages to R2 too, or installs will fail the hash check.')
  }

  console.log('\n── publish the index (user step) ──────────────────────────────')
  console.log('  wrangler kv key put index --path dist-plugins/index.json')
}

// Import-safe: vitest imports the exports above; only direct execution runs main.
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  await main()
}
