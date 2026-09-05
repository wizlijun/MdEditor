#!/usr/bin/env node
// Asserts that the Editor Kit really came out of the frontend build (spec §7).
//
// Why a build-output check and not a unit test: the failure mode is a *bundler*
// failure, invisible to any test that imports the source. It has already
// happened once — vite's app default for `preserveEntrySignatures` drops entry
// exports (HTML entries never need them), which tree-shook the whole kit down
// to a 35-byte file with its CSS side effect and nothing else. Every source
// test still passed; the kit only failed at runtime, inside a plugin webview,
// as a dynamic import that resolved to a module with no `mountMarkdownEditor`.
// A vitest case cannot see this without running a full `vite build` first, so
// the assertion is wired into `pnpm build` itself (see package.json) — that is
// also what `tauri build` runs via `beforeBuildCommand`, so no release can ship
// a broken kit.
//
// Checks, in order of what breaks in practice:
//   1. both artifacts exist under dist/assets/ with the stable, hash-free names
//      the `plugin://<id>/__host__/assets/…` URL hard-codes;
//   2. the JS is not a tree-shaken stub;
//   3. the JS actually exports `mountMarkdownEditor` (the v1 API entry point).

import { readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const JS = 'dist/assets/editor-kit-v1.js'
const CSS = 'dist/assets/editor-kit-v1.css'
const V2_JS = 'dist/assets/editor-kit-v2.js'

// A correct build is ~tens of KB of shell code; the known tree-shaken stub was
// 35 bytes. Anything under 1 KB is not a real kit.
const MIN_JS_BYTES = 1024

const problems = []

function sizeOf(rel) {
  try {
    return statSync(join(root, rel)).size
  } catch {
    return null
  }
}

const jsSize = sizeOf(JS)
const cssSize = sizeOf(CSS)
const v2JsSize = sizeOf(V2_JS)

if (jsSize === null) {
  problems.push(`${JS} is missing — the 'editor-kit' entry did not produce its bundle.`)
} else if (jsSize < MIN_JS_BYTES) {
  problems.push(
    `${JS} is only ${jsSize} bytes — the entry was almost certainly tree-shaken away ` +
      `(check rollupOptions.preserveEntrySignatures in vite.config.ts).`,
  )
} else {
  const js = readFileSync(join(root, JS), 'utf8')
  if (!js.includes('mountMarkdownEditor')) {
    problems.push(
      `${JS} does not export mountMarkdownEditor — plugins load it with ` +
        `await import('plugin://<id>/__host__/assets/editor-kit-v1.js') and call that export.`,
    )
  }
}

if (cssSize === null) {
  problems.push(`${CSS} is missing — the kit injects it by URL at mount time, so it must exist.`)
} else if (cssSize === 0) {
  problems.push(`${CSS} is empty — the kit would mount unstyled.`)
}

if (v2JsSize === null) {
  problems.push(`${V2_JS} is missing — the 'editor-kit-v2' entry did not produce its bundle.`)
} else if (v2JsSize < MIN_JS_BYTES) {
  problems.push(`${V2_JS} is only ${v2JsSize} bytes — the v2 entry was almost certainly tree-shaken away.`)
} else {
  const js = readFileSync(join(root, V2_JS), 'utf8')
  if (!js.includes('mountDocumentEditor')) {
    problems.push(`${V2_JS} does not export mountDocumentEditor.`)
  }
  if (!js.includes('documentEditorApiVersion')) {
    problems.push(`${V2_JS} does not export the document editor compatibility version.`)
  }
}

if (problems.length > 0) {
  console.error('Editor Kit build check FAILED:')
  for (const p of problems) console.error(`  - ${p}`)
  process.exit(1)
}

console.log(`Editor Kit build OK (${JS} ${jsSize} B, ${V2_JS} ${v2JsSize} B, ${CSS} ${cssSize} B)`)
