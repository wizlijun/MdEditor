#!/usr/bin/env node
// Browser acceptance for the real Editor Kit + Moraya surface. This is kept
// outside Vitest because clipboard defaults, contenteditable input and native
// selection need a browser. No synthetic ClipboardEvent or editor transaction
// is used for the user actions below.
//
// Run with an installed Playwright module, without changing project packages:
//   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node scripts/check-cdr-browser.mjs
// Set CDR_BROWSER_HEADED=1 to exercise a visible Chromium window.
// Set CDR_BROWSER_BUILT=1 after pnpm build to exercise the shipped JS entry.

import assert from 'node:assert/strict'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createServer } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')

async function loadPlaywright() {
  if (process.env.PLAYWRIGHT_MODULE) {
    return import(pathToFileURL(resolve(process.env.PLAYWRIGHT_MODULE)).href)
  }
  try {
    return await import('playwright')
  } catch {
    throw new Error('Install Playwright separately or set PLAYWRIGHT_MODULE to its index.mjs.')
  }
}

const entry = process.env.CDR_BROWSER_BUILT === '1' ? '/dist/assets/editor-kit-v2.js' : '/src/editor-kit-v2/main.ts'
const kitCssImport = process.env.CDR_BROWSER_BUILT === '1' ? '' : "import '/src/editor-kit/kit.css'"
const fixture = `<!doctype html>
<html><head><meta charset="utf-8"><title>CDR browser acceptance</title>
<style>body{margin:24px;font:16px system-ui}#editor{height:560px;border:1px solid #bbb}.ProseMirror{outline:none;min-height:500px}button{margin:8px}</style>
</head><body><h1>CDR browser acceptance</h1><div id="editor"></div><button id="blur">Outside editor</button>
<script type="module">
${kitCssImport}
import { mountDocumentEditor } from '${entry}'
import { applyDocumentChange } from '/src/lib/cdr/core.ts'
import { operationContentWrites } from '/src/lib/cdr/operation.ts'
window.notemd = { request: async (method) => method === 'host.vault.info' ? { root: '' } : {}, onMessage() {} }
let mounted = null
let generation = 0
let sequence = 0
let head = null
let history = []
let knownBlockIds = new Set()
let pending = []
let batches = []
let errors = []
let blocked = 0
let saveDelay = 20
let failNextSave = false
let surfaceState = null
let compositionEvents = []
for (const kind of ['compositionstart', 'compositionend']) {
  document.addEventListener(kind, (event) => compositionEvents.push({ kind, trusted: event.isTrusted }), true)
}
const next = (prefix) => prefix + '-' + (++sequence)
async function acknowledge(requestId, remoteContent = null) {
  const index = pending.findIndex((batch) => batch.requestId === requestId)
  if (index < 0) return
  const batch = pending.splice(index, 1)[0]
  try {
    if (failNextSave) {
      failNextSave = false
      await mounted.surface.reconcile({ kind: 'reject-local', requestId, authoritative: head, includedChangeIds: [],
        reason: { code: 'persistence-failed', message: 'Browser fixture simulated disk failure' } })
      return
    }
    const blockRevisions = Object.fromEntries(batch.operations.flatMap(operationContentWrites)
      .map((write) => [write.blockId, next('block-revision')]))
    head = applyDocumentChange(head, batch, { revisionId: next('revision'), blockRevisions }, { knownBlockIds, historicalRevisions: history })
    if (remoteContent !== null) {
      const block = head.blocks[0]
      head = applyDocumentChange(head, { documentId: head.documentId, requestId: next('remote-request'), baseRevisionId: head.revisionId,
        operations: [{ kind: 'block.replace', operationId: next('remote-operation'),
          target: { blockId: block.blockId, expectedBlockRevision: block.blockRevision }, payload: { content: remoteContent } }],
      }, { revisionId: next('remote-revision'), blockRevisions: { [block.blockId]: next('remote-block-revision') } })
    }
    for (const block of head.blocks) knownBlockIds.add(block.blockId)
    history.push(structuredClone(head))
    await mounted.surface.reconcile({ kind: 'ack-local', requestId, authoritative: head, includedChangeIds: [] })
  } catch (error) {
    errors.push(String(error))
  }
}
window.cdrTest = {
  async reset(markdowns = ['Alpha', 'Beta'], delay = 20, existing = null, savedHistory = null) {
    if (mounted) {
      try { await mounted.surface.flush?.() } catch { mounted.surface.discardDraft?.() }
      await mounted.surface.destroy()
    }
    const token = ++generation
    document.querySelector('#editor').replaceChildren()
    pending = []; batches = []; errors = []; blocked = 0; saveDelay = delay; failNextSave = false; surfaceState = null; compositionEvents = []
    head = existing ?? { documentId: 'browser-document', revisionId: next('revision'),
      blocks: markdowns.map((markdown, index) => ({ blockId: 'block-' + index, blockRevision: next('block-revision'), markdown })) }
    history = savedHistory ?? [structuredClone(head)]
    knownBlockIds = new Set(history.flatMap((revision) => revision.blocks.map((block) => block.blockId)))
    mounted = await mountDocumentEditor(document.querySelector('#editor'), {
      snapshot: head,
      ids: { requestId: () => next('request'), operationId: () => next('operation'), blockId: () => next('block') },
      localChangeDebounceMs: 30,
      onBlockedStructuralEdit() { blocked++ },
      onResyncRequired(reason) { errors.push(JSON.stringify(reason)) },
    })
    mounted.surface.observeLocalOperations((batch) => {
      if (token !== generation) return
      pending.push(batch); batches.push(batch)
      if (saveDelay >= 0) setTimeout(() => { if (token === generation) void acknowledge(batch.requestId) }, saveDelay)
    })
    mounted.surface.observeState?.((value) => { surfaceState = value })
  },
  state() {
    const editor = document.querySelector('.ProseMirror')
    return { head, pending: pending.length, batches, errors, blocked, surfaceState, compositionEvents, text: editor?.innerText ?? '',
      editable: editor?.getAttribute('contenteditable'), focused: document.activeElement === editor,
      selection: String(window.getSelection()), html: editor?.innerHTML ?? '' }
  },
  async settle() {
    for (let count = 0; count < 20; count++) {
      await new Promise((resolve) => setTimeout(resolve, 50))
      if (pending.length) await acknowledge(pending[0].requestId)
    }
  },
  async reopen() {
    const saved = structuredClone(head)
    await this.reset([], saveDelay, saved, structuredClone(history))
  },
  failNextSave() { failNextSave = true },
  draft() { return mounted.surface.getDraftMarkdown() },
  retry() { return mounted.surface.retryPending() },
  flush() { return mounted.surface.flush() },
  command(command) { return mounted.surface.executeCommand(command) },
  structuralCommand(command) { return mounted.surface.executeStructuralCommand(command) },
  restore(index) { return mounted.surface.restoreRevision(history[index]) },
  discard() { return mounted.surface.discardDraft() },
  saveReloadHead() { sessionStorage.setItem('cdr-browser-reload', JSON.stringify({ head, history })) },
  ackWithRemote(content) { return acknowledge(pending[0].requestId, content) },
  async remoteMove(blockId, index) {
    const before = head
    const old = head.blocks.findIndex((block) => block.blockId === blockId)
    const block = head.blocks[old]
    const rest = head.blocks.filter((block) => block.blockId !== blockId)
    const operation = { kind: 'block.move', operationId: next('remote-operation'),
      target: { blockId, expectedBlockRevision: block.blockRevision },
      payload: {
        source: { leftBlockId: head.blocks[old - 1]?.blockId ?? null, rightBlockId: head.blocks[old + 1]?.blockId ?? null },
        destination: { leftBlockId: rest[index - 1]?.blockId ?? null, rightBlockId: rest[index]?.blockId ?? null },
      },
    }
    const change = { changeId: next('remote-change'), baseRevisionId: before.revisionId,
      revisionId: next('remote-revision'), blockRevisions: {}, operations: [operation] }
    head = applyDocumentChange(before, { requestId: change.changeId, documentId: before.documentId,
      baseRevisionId: before.revisionId, operations: [operation] }, change, { knownBlockIds, historicalRevisions: history })
    history.push(structuredClone(head))
    await mounted.surface.reconcile({ kind: 'apply-remote', change })
  },
  selectText(fromNode, fromOffset, toNode = fromNode, toOffset = fromOffset) {
    const editor = document.querySelector('.ProseMirror')
    editor.focus()
    const walker = document.createTreeWalker(editor, NodeFilter.SHOW_TEXT)
    const nodes = []
    while (walker.nextNode()) nodes.push(walker.currentNode)
    const range = document.createRange()
    range.setStart(nodes[fromNode], fromOffset)
    range.setEnd(nodes[toNode], toOffset)
    const selection = window.getSelection()
    selection.removeAllRanges(); selection.addRange(range)
  },
}
const reloadHead = JSON.parse(sessionStorage.getItem('cdr-browser-reload') ?? 'null')
sessionStorage.removeItem('cdr-browser-reload')
if (reloadHead) await window.cdrTest.reset([], 20, reloadHead.head, reloadHead.history)
else await window.cdrTest.reset()
window.cdrReady = true
</script></body></html>`

const memoryFixture = `<!doctype html>
<html><head><meta charset="utf-8"><title>Memory component browser acceptance</title>
<style>#component-root{max-width:1240px;margin:auto;padding:20px 24px 40px}</style>
</head><body><div id="memory-app"><div id="component-root"></div></div><script type="module">
import '/src/styles/app.css'
import '/plugins-src/memory/src/App.svelte'
${kitCssImport}
import { mount, unmount } from 'svelte'
import CollaborativeDocument from '/plugins-src/memory/src/lib/CollaborativeDocumentSpike.svelte'
import { sha256Hex } from '/plugins-src/memory/src/lib/cdr/session.ts'
import { DOCUMENT_AGENT_TASK } from '/plugins-src/memory/src/lib/document-agent.ts'
let repository = null
let component = null
let failNext = false
const errors = []
const agent = { id: 'notemd.codex-agent', name: 'Codex Agent', harness: { harness: 'Codex', ok: true, capabilities: {
  tasks: [DOCUMENT_AGENT_TASK], search_plan_schemas: [1], terminal_result: true, input_only_isolation: true,
  model_routing: { invocation_override: true, profiles: { fast: { available: true }, default: { available: true } }, selectable_models: [] },
} } }
const mountComponent = () => mount(CollaborativeDocument, { target: document.querySelector('#component-root'), props: { agent } })
window.notemd = { pluginId: 'notemd.memory', locale: 'zh-CN', theme: 'light', onMessage() {}, async request(method, params = {}) {
  if (method === 'host.theme.css') return {}
  if (method === 'host.vault.info') return { root: '/browser-fixture', wiki_dir: 'wiki', author: 'human:browser-tester' }
  if (method === 'host.clipboard.write') { await navigator.clipboard.writeText(params.text); return { ok: true } }
  if (method === 'host.agent.run') return { run_id: 'browser-agent-run' }
  if (method === 'host.agent.status') return { state: 'done', record: { status: 'success', result: 'complete', harness: 'notemd.codex-agent' },
    terminal_result: { complete: true, content: JSON.stringify({ schema: 'notemd.cdr/agent-result/v1', kind: 'suggestion',
      content: 'Agent replaced the selected paragraph.', summary: 'Browser fixture proposal' }) } }
  if (method === 'host.cdr.repository.v2.inspect') return repository ? { kind: 'located', document_id: repository.documentId } : { kind: 'missing' }
  if (method === 'host.cdr.repository.v2.load') return repository ? {
    kind: 'loaded', generation: repository.generation, aggregate: structuredClone(repository.aggregate),
    representation: { vault_path: params.vault_path, committed_sha256: await sha256Hex(repository.markdown),
      status: 'in-sync', disk_sha256: await sha256Hex(repository.markdown), markdown: repository.markdown, profile_type: 'Memory' },
  } : { kind: 'missing' }
  if (method === 'host.cdr.repository.v2.commit') {
    if (failNext) { failNext = false; throw new Error('Browser fixture simulated disk failure') }
    const generation = repository?.generation ?? 0
    if (params.expected_generation !== generation) throw new Error('Browser fixture CAS mismatch')
    const expected = params.representation.expected.kind === 'present' ? params.representation.expected.sha256 : null
    if (expected !== (repository ? await sha256Hex(repository.markdown) : null)) throw new Error('Browser fixture representation mismatch')
    await new Promise((resolve) => setTimeout(resolve, 50))
    repository = { generation: generation + 1, aggregate: structuredClone(params.aggregate), markdown: params.representation.markdown, documentId: params.document_id }
    return { kind: 'committed', generation: repository.generation, representation_sha256: await sha256Hex(repository.markdown) }
  }
  errors.push('Unexpected Host method: ' + method)
  throw new Error(errors.at(-1))
} }
window.memoryUiTest = {
  state() {
    const editor = document.querySelector('.ProseMirror')
    return { repository, errors, status: document.querySelector('.status')?.textContent ?? '', text: editor?.innerText ?? '',
      draft: window.memorySurface?.getDraftMarkdown(), surfaceState: window.memorySurfaceState,
      pending: window.memoryPending?.size ?? 0, document: editor?.pmViewDesc?.node?.toJSON() }
  },
  select(blockId, start, end) {
    const editor = document.querySelector('.ProseMirror'); editor.focus()
    const block = editor.querySelector('[data-cdr-block-id="' + blockId + '"]')
    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT)
    walker.nextNode()
    const range = document.createRange(); range.setStart(walker.currentNode, start); range.setEnd(walker.currentNode, end)
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range)
  },
  selectEnd() {
    const editor = document.querySelector('.ProseMirror'); editor.focus()
    const range = document.createRange(); range.selectNodeContents(editor); range.collapse(false)
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range)
  },
  failNextSave() { failNext = true },
  async reopen() { await unmount(component); component = mountComponent() },
}
component = mountComponent()
window.memoryUiReady = true
</script></body></html>`

const { chromium } = await loadPlaywright()
const server = await createServer({
  root,
  resolve: { dedupe: ['svelte'] },
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false, watch: null },
  plugins: [{
    name: 'cdr-browser-fixture',
    transform(code, id) {
      if (!id.endsWith('/plugins-src/memory/src/lib/editor-kit-v2.ts')) return null
      // Adapt only the transport URL: the actual component, session,
      // repository and Editor Kit implementations still run unchanged.
      return code.replace(/return \x60plugin:[^\n]+\x60/, 'return ' + JSON.stringify(entry))
        .replace(/return mod\.mountDocumentEditor(?: as MountDocumentEditor)?;?/, `return (async (container, options) => {
          const mounted = await mod.mountDocumentEditor(container, options)
          window.memorySurface = mounted.surface
          window.memoryPending = new Set()
          mounted.surface.observeState((state) => { window.memorySurfaceState = state })
          mounted.surface.observeLocalOperations((batch) => { window.memoryPending.add(batch.requestId) })
          const reconcile = mounted.surface.reconcile.bind(mounted.surface)
          mounted.surface.reconcile = async (update) => {
            await reconcile(update)
            if (update.requestId) window.memoryPending.delete(update.requestId)
          }
          return mounted
        })`)
    },
    configureServer(vite) {
      vite.middlewares.use('/__cdr-memory-ui', async (req, res, next) => {
        if (req.url?.includes('html-proxy')) { next(); return }
        res.setHeader('Content-Type', 'text/html; charset=utf-8')
        res.end(await vite.transformIndexHtml('/__cdr-memory-ui', memoryFixture))
      })
      vite.middlewares.use('/__cdr-browser', (_req, res) => {
        res.setHeader('Content-Type', 'text/html; charset=utf-8')
        res.end(fixture)
      })
      // The source entry uses the production CSS URL. The fixture imports
      // that same stylesheet above; keep this additional URL noise-free.
      vite.middlewares.use('/src/editor-kit-v2/editor-kit-v1.css', (_req, res) => {
        res.setHeader('Content-Type', 'text/css')
        res.end('')
      })
    },
  }],
})
let browser
let failures = 0
let restoreClipboard = async () => {}
const results = []
try {
  await server.listen()
  const address = server.httpServer.address()
  const origin = 'http://127.0.0.1:' + address.port
  browser = await chromium.launch({ headless: process.env.CDR_BROWSER_HEADED !== '1' })
  const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] })
  const failedResponses = []
  context.on('response', (response) => {
    if (response.status() >= 400) failedResponses.push({ status: response.status(), url: response.url() })
  })
  const page = await context.newPage()
  const pageErrors = []
  page.on('pageerror', (error) => pageErrors.push(error.message))
  await page.goto(origin + '/__cdr-browser')
  try {
    await page.waitForFunction(() => window.cdrReady === true)
  } catch (error) {
    throw new Error('Browser fixture failed to mount: ' + (pageErrors.join('; ') || error.message))
  }
  // Keep clipboard contents inside the browser, including supported rich
  // formats. They are never printed or included in the test report.
  await page.evaluate(async () => { window.savedClipboard = await navigator.clipboard.read() })
  restoreClipboard = () => page.evaluate(async () => {
    if (window.savedClipboard.length) await navigator.clipboard.write(window.savedClipboard)
    else await navigator.clipboard.writeText('')
  })

  const editor = page.locator('.ProseMirror')
  const mod = process.platform === 'darwin' ? 'Meta' : 'Control'
  const state = () => page.evaluate(() => window.cdrTest.state())
  const reset = (markdowns, delay = 20) => page.evaluate(({ markdowns, delay }) => window.cdrTest.reset(markdowns, delay), { markdowns, delay })
  const settle = async () => {
    await page.evaluate(() => window.cdrTest.settle())
    const current = await state()
    assert.deepEqual(current.errors, [], 'persistence/reconciliation errors')
    assert.equal(current.pending, 0, 'pending operations drained')
    return current
  }
  const select = async (...args) => {
    await page.evaluate((args) => window.cdrTest.selectText(...args), args)
    // Native selectionchange is asynchronous; allow the editor to observe it.
    await page.waitForTimeout(30)
  }
  const test = async (name, body, describeState = state) => {
    if (process.env.CDR_BROWSER_FILTER && !name.includes(process.env.CDR_BROWSER_FILTER)) return
    try {
      await body()
      results.push({ name, status: 'passed' })
      console.log('PASS ' + name)
    } catch (error) {
      failures++
      results.push({ name, status: 'failed', error: error.message, state: await describeState() })
      console.error('FAIL ' + name + ': ' + error.message)
    }
  }

  await test('Enter, Shift+Enter, and insertion persist across reopen', async () => {
    await reset(['Alpha', 'Beta'])
    await select(0, 5)
    await page.keyboard.press('Enter')
    await page.keyboard.type('Bravo')
    await page.keyboard.press('Shift+Enter')
    await page.keyboard.type('Charlie')
    const current = await settle()
    const markdown = current.head.blocks.map((block) => block.markdown).join('\n\n')
    assert.match(markdown, /Alpha\s+Bravo/)
    assert.match(markdown, /Charlie/)
    assert.equal(current.head.blocks.find((block) => block.blockId === 'block-1').markdown, 'Beta')
    await page.evaluate(() => window.cdrTest.reopen())
    assert.match((await state()).text, /Charlie/)
  })

  await test('two trailing Enter paragraphs save immediately and retain their identities on reopen', async () => {
    await reset(['Alpha', 'Beta'])
    await select(1, 4)
    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.evaluate(() => window.cdrTest.flush())
    const saved = await state()
    assert.deepEqual(saved.head.blocks.map((block) => block.markdown), ['Alpha', 'Beta', '', ''])
    assert.equal(new Set(saved.head.blocks.map((block) => block.blockId)).size, 4)
    assert.equal(await editor.locator('p').count(), 4)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, saved.head)
    assert.equal(await editor.locator('p').count(), 4)
    assert.equal((await state()).surfaceState.dirty, false)
  })

  await test('a trailing Shift+Enter hardbreak survives immediate flush and reopen', async () => {
    await reset(['Alpha', 'Beta'])
    await select(1, 4)
    await page.keyboard.press('Shift+Enter')
    await page.evaluate(() => window.cdrTest.flush())
    const saved = await state()
    assert.equal(await editor.locator('.hardbreak-marker').count(), 1)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, saved.head)
    assert.equal(await editor.locator('.hardbreak-marker').count(), 1)
    assert.equal((await state()).surfaceState.dirty, false)
  })

  await test('native copy, cut, and paste preserve clipboard bytes and text', async () => {
    await reset(['Alpha Bravo', 'Beta'])
    await select(0, 0, 0, 5)
    await page.keyboard.press(mod + '+c')
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), 'Alpha')
    await page.keyboard.press(mod + '+x')
    let current = await settle()
    assert.equal(current.head.blocks[0].markdown.trim(), 'Bravo')
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), 'Alpha')
    await select(0, 0)
    await page.keyboard.press(mod + '+v')
    current = await settle()
    assert.match(current.head.blocks[0].markdown, /Alpha.*Bravo/)
  })

  await test('native multiline paste retains both paragraphs', async () => {
    await reset(['Alpha', 'Beta'])
    await page.evaluate(() => navigator.clipboard.writeText('粘贴第一段\n\n粘贴第二段'))
    await select(0, 5)
    await page.keyboard.press(mod + '+v')
    const current = await settle()
    assert.match(current.head.blocks[0].markdown, /粘贴第一段/)
    assert.match(current.head.blocks[0].markdown, /粘贴第二段/)
    assert.equal(current.head.blocks[1].markdown, 'Beta')
  })

  await test('typing continues while persistence acknowledgement is pending', async () => {
    await reset(['Alpha', 'Beta'], 600)
    await select(0, 5)
    await page.keyboard.type('1')
    await page.waitForFunction(() => window.cdrTest.state().pending > 0)
    assert.equal((await state()).editable, 'true', 'editor stays editable during save')
    await page.keyboard.type('2345', { delay: 60 })
    const current = await settle()
    assert.equal(current.head.blocks[0].markdown, 'Alpha12345')
  })

  await test('browser IME composition commits Chinese text while a save is pending', async () => {
    await reset(['Alpha', 'Beta'], 600)
    await select(0, 5)
    await page.keyboard.type('1')
    await page.waitForFunction(() => window.cdrTest.state().pending > 0)
    const cdp = await context.newCDPSession(page)
    try {
      await cdp.send('Input.imeSetComposition', { text: '中', selectionStart: 1, selectionEnd: 1 })
      await cdp.send('Input.imeSetComposition', { text: '中文', selectionStart: 2, selectionEnd: 2 })
      await cdp.send('Input.insertText', { text: '中文' })
    } finally {
      await cdp.detach()
    }
    const current = await settle()
    assert.equal(current.head.blocks[0].markdown, 'Alpha1中文')
    assert.deepEqual(current.compositionEvents.map((event) => event.kind), ['compositionstart', 'compositionend'])
    assert.equal(current.compositionEvents[0].trusted, true, 'composition starts in the browser input subsystem')
  })

  await test('an acknowledgement containing a same-block remote edit preserves the composing draft', async () => {
    await reset(['Alpha', 'Beta'], -1)
    await select(0, 5)
    await page.keyboard.type('1')
    await page.waitForFunction(() => window.cdrTest.state().pending > 0)
    const cdp = await context.newCDPSession(page)
    try {
      await cdp.send('Input.imeSetComposition', { text: '中', selectionStart: 1, selectionEnd: 1 })
      await page.waitForTimeout(40)
      await page.evaluate(() => window.cdrTest.ackWithRemote('Server changed this block'))
      await cdp.send('Input.imeSetComposition', { text: '中文', selectionStart: 2, selectionEnd: 2 })
      await cdp.send('Input.insertText', { text: '中文' })
    } finally {
      await cdp.detach()
    }
    await page.waitForTimeout(80)
    const current = await state()
    assert.equal(current.head.blocks[0].markdown, 'Server changed this block')
    assert.match(await page.evaluate(() => window.cdrTest.draft()), /Alpha1中文/)
    assert.ok(current.surfaceState.error, 'same-block disagreement is visible for resolution')
    assert.equal(current.pending, 0, 'conflicting candidate is not silently submitted')
    await page.evaluate(() => window.cdrTest.discard())
  })

  await test('cross-block selection replacement preserves remaining text', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    await select(0, 3, 1, 2)
    await page.keyboard.type('X')
    const current = await settle()
    assert.match(current.text, /AlpXta/)
    assert.equal(current.head.blocks.at(-1).markdown, 'Gamma')
    assert.equal(current.blocked, 0)
  })

  await test('Select All, cut, empty save, and paste recover a usable document', async () => {
    await reset(['Alpha', 'Beta'])
    await editor.focus()
    await page.keyboard.press(mod + '+a')
    await page.keyboard.press(mod + '+x')
    let current = await settle()
    assert.equal(current.text.trim(), '')
    const copied = await page.evaluate(() => navigator.clipboard.readText())
    assert.match(copied, /Alpha/)
    assert.match(copied, /Beta/)
    await editor.focus()
    await page.keyboard.press(mod + '+v')
    current = await settle()
    assert.match(current.text, /Alpha/)
    assert.match(current.text, /Beta/)
  })

  await test('Select All and Backspace can be saved and typed into again', async () => {
    await reset(['Alpha', 'Beta'])
    await editor.focus()
    await page.keyboard.press(mod + '+a')
    await page.keyboard.press('Backspace')
    await settle()
    assert.equal((await state()).text.trim(), '')
    await editor.focus()
    await page.keyboard.type('New document')
    const current = await settle()
    assert.equal(current.text.trim(), 'New document')
    assert.equal(current.head.blocks.map((block) => block.markdown).join('\n').trim(), 'New document')
  })

  await test('Select All followed by ArrowRight collapses before subsequent typing', async () => {
    await reset(['Alpha', 'Beta'])
    await editor.focus()
    await page.keyboard.press(mod + '+a')
    await page.keyboard.press('ArrowRight')
    await page.waitForTimeout(40)
    await page.keyboard.type('Tail')
    const current = await settle()
    assert.equal(current.head.blocks[0].markdown, 'Alpha')
    assert.equal(current.head.blocks[1].markdown, 'BetaTail')
  })

  await test('leading-space Markdown reopens as saved without a phantom dirty state', async () => {
    await reset([' Leading space', 'Beta'])
    const current = await state()
    assert.equal(current.surfaceState.dirty, false)
    assert.equal(current.pending, 0)
    assert.equal(current.head.blocks[0].markdown, ' Leading space')
    await page.evaluate(() => window.cdrTest.reopen())
    assert.equal((await state()).surfaceState.dirty, false)
  })

  await test('undo and redo survive an acknowledged paragraph edit', async () => {
    await reset(['Alpha', 'Beta'])
    await select(0, 5)
    await page.keyboard.press('Enter')
    await page.keyboard.type('Bravo')
    await settle()
    await page.keyboard.press(mod + '+z')
    await settle()
    assert.doesNotMatch((await state()).text, /Bravo/)
    await page.keyboard.press(mod + '+Shift+z')
    const current = await settle()
    assert.match(current.text, /Bravo/)
  })

  await test('moving a different block remotely preserves the edited block undo history', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    await select(0, 5)
    await page.keyboard.type(' edited')
    await settle()
    await page.evaluate(() => window.cdrTest.remoteMove('block-1', 0))
    await page.keyboard.press(mod + '+z')
    const current = await settle()
    assert.equal(current.head.blocks.find((block) => block.blockId === 'block-0').markdown, 'Alpha')
    assert.deepEqual(current.head.blocks.map((block) => block.blockId), ['block-1', 'block-0', 'block-2'])
  })

  await test('moving the edited block remotely preserves its undo history', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    await select(0, 5)
    await page.keyboard.type(' edited')
    await settle()
    await page.evaluate(() => window.cdrTest.remoteMove('block-0', 2))
    await page.keyboard.press(mod + '+z')
    const current = await settle()
    assert.equal(current.head.blocks.find((block) => block.blockId === 'block-0').markdown, 'Alpha')
    assert.deepEqual(current.head.blocks.map((block) => block.blockId), ['block-1', 'block-2', 'block-0'])
  })

  await test('moving the edited block remotely to the front preserves its undo history', async () => {
    await reset(['Beta', 'Gamma', 'Alpha'])
    await select(2, 5)
    await page.keyboard.type(' edited')
    await settle()
    await page.evaluate(() => window.cdrTest.remoteMove('block-2', 0))
    await page.keyboard.press(mod + '+z')
    const current = await settle()
    assert.equal(current.head.blocks.find((block) => block.blockId === 'block-2').markdown, 'Alpha')
    assert.deepEqual(current.head.blocks.map((block) => block.blockId), ['block-2', 'block-0', 'block-1'])
  })

  await test('format commands produce durable Markdown through the real editor', async () => {
    const cases = [
      [{ kind: 'heading', level: 2 }, /^## Alpha$/],
      [{ kind: 'bold' }, /^\*\*Alpha\*\*$/],
      [{ kind: 'italic' }, /^(?:\*Alpha\*|_Alpha_)$/],
      [{ kind: 'blockquote' }, /^> Alpha$/],
      [{ kind: 'task-list' }, /^[-*] \[ \] Alpha$/],
      [{ kind: 'code-block' }, /^\x60{3}[^\n]*\nAlpha\n\x60{3}$/],
      [{ kind: 'link', href: 'https://example.invalid/doc' }, /^\[Alpha\]\(https:\/\/example\.invalid\/doc\)$/],
    ]
    for (const [command, expected] of cases) {
      await reset(['Alpha', 'Beta'])
      await select(0, 0, 0, 5)
      assert.equal(await page.evaluate((command) => window.cdrTest.command(command), command), true, command.kind)
      const current = await settle()
      assert.match(current.head.blocks[0].markdown, expected, command.kind)
      assert.equal(current.head.blocks[1].markdown, 'Beta')
    }
  })

  await test('list formatting accepts a selection across existing knowledge blocks', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    await select(0, 0, 1, 4)
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'bullet-list' })), true)
    const current = await settle()
    const markdown = current.head.blocks.map((block) => block.markdown).join('\n\n')
    assert.match(markdown, /[-*] Alpha/)
    assert.match(markdown, /[-*] Beta/)
    assert.equal(current.head.blocks.at(-1).markdown, 'Gamma')
    assert.equal(current.blocked, 0)
  })

  await test('switching list styles keeps one list and toggling it off restores a paragraph', async () => {
    await reset(['Alpha', 'Beta'])
    await select(0, 0, 0, 5)
    for (const kind of ['bullet-list', 'ordered-list', 'task-list', 'bullet-list']) {
      assert.equal(await page.evaluate((kind) => window.cdrTest.command({ kind }), kind), true)
      await settle()
      assert.equal(await editor.locator('ul, ol').count(), 1, kind + ' must not nest another list')
    }
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'bullet-list' })), true)
    const current = await settle()
    assert.equal(await editor.locator('ul, ol').count(), 0)
    assert.equal(current.head.blocks[0].markdown, 'Alpha')
  })

  await test('table insertion, cell typing, row and column changes persist', async () => {
    await reset(['Alpha', 'Beta'])
    await select(0, 0, 0, 5)
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'table' })), true)
    await editor.locator('th').first().click()
    await page.keyboard.type('Column')
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'table.next-cell' })), true)
    await page.keyboard.type('Next')
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'table.add-row' })), true)
    assert.equal(await page.evaluate(() => window.cdrTest.command({ kind: 'table.add-column' })), true)
    const current = await settle()
    assert.match(current.head.blocks[0].markdown, /Column/)
    assert.match(current.head.blocks[0].markdown, /Next/)
    assert.equal(await editor.locator('tr').count(), 4)
    assert.equal(await editor.locator('th').count(), 4)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.equal(await editor.locator('tr').count(), 4)
    assert.equal(await editor.locator('th').count(), 4)
  })

  await test('moving a knowledge block preserves its identity and content', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    assert.equal(await page.evaluate(() => window.cdrTest.structuralCommand({ kind: 'block.move-up', blockId: 'block-1' })), true)
    const current = await settle()
    assert.deepEqual(current.head.blocks.map((block) => [block.blockId, block.markdown]), [
      ['block-1', 'Beta'], ['block-0', 'Alpha'], ['block-2', 'Gamma'],
    ])
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, current.head)
  })

  await test('restoring history recovers deleted blocks with their original identities', async () => {
    await reset(['Alpha', 'Beta', 'Gamma'])
    const original = (await state()).head
    assert.equal(await page.evaluate(() => window.cdrTest.structuralCommand({ kind: 'block.delete', blockId: 'block-1' })), true)
    const deleted = await settle()
    assert.deepEqual(deleted.head.blocks.map((block) => block.blockId), ['block-0', 'block-2'])
    assert.equal(await page.evaluate(() => window.cdrTest.restore(0)), true)
    const restored = await settle()
    assert.deepEqual(restored.head.blocks.map((block) => [block.blockId, block.markdown]), original.blocks.map((block) => [block.blockId, block.markdown]))
    assert.notEqual(restored.head.revisionId, original.revisionId)
    assert.notEqual(restored.head.revisionId, deleted.head.revisionId)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, restored.head)
  })

  await test('failed persistence retains the exact draft and retry saves it', async () => {
    await reset(['Alpha', 'Beta'])
    await page.evaluate(() => window.cdrTest.failNextSave())
    await select(0, 5)
    await page.keyboard.type(' unsaved draft')
    await page.waitForFunction(() => window.cdrTest.state().surfaceState?.error != null)
    let current = await state()
    assert.equal(current.head.blocks[0].markdown, 'Alpha', 'failure does not advance the durable head')
    assert.match(current.text, /Alpha unsaved draft/, 'failure does not discard user text')
    assert.match(await page.evaluate(() => window.cdrTest.draft()), /Alpha unsaved draft/)
    await page.evaluate(() => window.cdrTest.retry())
    current = await settle()
    assert.equal(current.head.blocks[0].markdown, 'Alpha unsaved draft')
    assert.equal(current.surfaceState.error, null)
    const saved = structuredClone(current.head)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, saved, 'reopen keeps block and revision identities')
    assert.match((await state()).text, /Alpha unsaved draft/)
  })

  await test('a failed draft survives browser reload and waits for an explicit retry', async () => {
    const recovery = await context.newPage()
    recovery.on('pageerror', (error) => pageErrors.push(error.message))
    try {
      await recovery.goto(origin + '/__cdr-browser')
      await recovery.waitForFunction(() => window.cdrReady === true)
      await recovery.evaluate(() => window.cdrTest.reset([], 20, {
        documentId: 'reload-document', revisionId: 'reload-revision-1',
        blocks: [{ blockId: 'reload-block', blockRevision: 'reload-block-revision-1', markdown: 'Alpha' }],
      }))
      await recovery.evaluate(() => { window.cdrTest.failNextSave(); window.cdrTest.selectText(0, 5) })
      await recovery.waitForTimeout(40)
      await recovery.keyboard.type(' retained through reload')
      await recovery.waitForFunction(() => window.cdrTest.state().surfaceState.error !== null)
      const before = await recovery.evaluate(() => window.cdrTest.state())
      await recovery.evaluate(() => window.cdrTest.saveReloadHead())
      await recovery.reload()
      await recovery.waitForFunction(() => window.cdrReady === true)
      await recovery.waitForTimeout(400)
      const after = await recovery.evaluate(() => window.cdrTest.state())
      assert.deepEqual(after.head, before.head)
      assert.match(after.text, /Alpha retained through reload/)
      assert.ok(after.surfaceState.error, 'recovered content is visibly awaiting user action')
      assert.equal(after.pending, 0)
      assert.equal(after.batches.length, 0, 'reopen does not submit the recovered draft automatically')
      await recovery.evaluate(() => window.cdrTest.retry())
      await recovery.evaluate(() => window.cdrTest.settle())
      const saved = await recovery.evaluate(() => window.cdrTest.state())
      assert.equal(saved.head.blocks[0].markdown, 'Alpha retained through reload')
      assert.equal(saved.surfaceState.error, null)
    } finally {
      await recovery.evaluate(() => window.cdrTest?.discard()).catch(() => {})
      await recovery.close()
    }
  })

  await test('flush waits for acknowledgement and reopened state has the final text', async () => {
    await reset(['Alpha', 'Beta'], 300)
    await select(0, 5)
    await page.keyboard.type(' final')
    await page.evaluate(() => {
      window.flushFinished = false
      window.flushError = null
      window.cdrTest.flush().then(() => { window.flushFinished = true }, (error) => { window.flushError = String(error) })
    })
    assert.equal(await page.evaluate(() => window.flushFinished), false)
    await page.waitForFunction(() => window.flushFinished || window.flushError)
    assert.equal(await page.evaluate(() => window.flushError), null)
    const current = await state()
    assert.equal(current.head.blocks[0].markdown, 'Alpha final')
    assert.equal(current.pending, 0)
    assert.equal(current.surfaceState.dirty, false)
    await page.evaluate(() => window.cdrTest.reopen())
    assert.deepEqual((await state()).head, current.head)
    assert.match((await state()).text, /Alpha final/)
  })

  await test('actual Memory toolbar, address form, recovery and reopen work together', async () => {
    const ui = await context.newPage()
    ui.on('pageerror', (error) => pageErrors.push(error.message))
    let phase = 'mount'
    try {
      await ui.goto(origin + '/__cdr-memory-ui')
      await ui.waitForFunction(() => window.memoryUiReady === true)
      await ui.getByRole('button', { name: '创建受控 MEMORY 文档', exact: true }).click()
      await ui.locator('.ProseMirror').waitFor()
      await ui.waitForFunction(() => window.memoryUiTest.state().status === '已保存')
      const selectUi = async (blockId, start, end) => {
        await ui.evaluate(({ blockId, start, end }) => window.memoryUiTest.select(blockId, start, end), { blockId, start, end })
        await ui.waitForTimeout(40)
      }

      await selectUi('b-d4e5f6', 0, 4)
      await ui.getByRole('button', { name: '粗体', exact: true }).click()
      await ui.waitForFunction(() => window.memoryUiTest.state().repository.aggregate.session.head.blocks
        .find((block) => block.blockId === 'b-d4e5f6').markdown.includes('**'))

      await selectUi('b-0a1b2c', 0, 5)
      await ui.getByRole('button', { name: '插入链接', exact: true }).click()
      await ui.getByLabel('链接地址', { exact: true }).fill('https://example.invalid/context')
      await ui.getByRole('button', { name: '确认插入', exact: true }).click()
      await ui.waitForFunction(() => window.memoryUiTest.state().repository.aggregate.session.head.blocks
        .find((block) => block.blockId === 'b-0a1b2c').markdown.includes('[Agent](https://example.invalid/context)'))

      // Use native selection at document end after formatting, avoiding
      // implementation-specific decoration text nodes.
      await ui.evaluate(() => window.memoryUiTest.selectEnd())
      await ui.waitForTimeout(40)
      await ui.evaluate(() => window.memoryUiTest.failNextSave())
      await ui.keyboard.type(' Browser failure draft')
      phase = 'show failure'
      await ui.getByRole('button', { name: '重试保存', exact: true }).waitFor()
      assert.match(await ui.locator('.ProseMirror').innerText(), /Browser failure draft/)
      await ui.getByRole('button', { name: '复制草稿', exact: true }).click()
      assert.match(await ui.evaluate(() => navigator.clipboard.readText()), /Browser failure draft/)
      await ui.getByRole('button', { name: '重试保存', exact: true }).click()
      phase = 'retry commit'
      await ui.waitForFunction(() => window.memoryUiTest.state().status === '已保存')
      const saved = await ui.evaluate(() => window.memoryUiTest.state())
      assert.deepEqual(saved.errors, [])
      assert.match(saved.repository.markdown, /Browser failure draft/)
      await ui.evaluate(() => window.memoryUiTest.reopen())
      phase = 'reopen saved component'
      await ui.waitForFunction(() => window.memoryUiTest.state().status === '已保存')
      assert.deepEqual(await ui.evaluate(() => window.memoryUiTest.state().repository), saved.repository)
      assert.match(await ui.locator('.ProseMirror').innerText(), /Browser failure draft/)

      phase = 'link form target updated by an accepted Agent proposal'
      await selectUi('b-d4e5f6', 0, 4)
      await ui.getByRole('button', { name: '插入链接', exact: true }).click()
      await ui.getByRole('button', { name: '让 Agent 建议改写选中块', exact: true }).click()
      await ui.getByRole('button', { name: '接受', exact: true }).click()
      await ui.waitForFunction(() => window.memoryUiTest.state().repository.aggregate.session.head.blocks
        .find((block) => block.blockId === 'b-d4e5f6').markdown === 'Agent replaced the selected paragraph.')
      const remoteGeneration = await ui.evaluate(() => window.memoryUiTest.state().repository.generation)
      await ui.getByLabel('链接地址', { exact: true }).fill('https://example.invalid/stale-selection')
      await ui.getByRole('button', { name: '确认插入', exact: true }).click()
      await ui.waitForTimeout(500)
      const afterStaleLink = await ui.evaluate(() => window.memoryUiTest.state())
      assert.equal(afterStaleLink.repository.generation, remoteGeneration, 'stale form must not create a new revision')
      assert.doesNotMatch(afterStaleLink.repository.markdown, /stale-selection/)
      assert.equal(await ui.getByLabel('链接地址', { exact: true }).count(), 0)
      assert.equal(afterStaleLink.status, '已保存', 'rejecting a stale form does not leave a phantom draft')
      assert.equal(await ui.getByRole('link', { name: 'Agent', exact: true }).count(), 1, 'an untouched link stays a real link after remote edits')
      assert.ok(!afterStaleLink.draft.includes('\\[Agent\\]'), 'link syntax must not turn into escaped literal text')

      phase = 'scroll to lower history and assessment controls'
      await ui.mouse.move(1200, 680)
      await ui.mouse.wheel(0, 1200)
      await ui.waitForTimeout(100)
      assert.ok(await ui.evaluate(() => document.querySelector('#memory-app').scrollTop > 0), 'plugin root scrolls beyond the viewport')
      const aside = await ui.locator('.workbench aside').evaluate((node) => ({ left: node.scrollLeft, width: node.clientWidth, contentWidth: node.scrollWidth }))
      assert.equal(aside.left, 0, 'native scrolling does not move sidebar controls sideways')
      assert.ok(aside.contentWidth <= aside.width, 'long actor and revision identifiers wrap in the sidebar')
      await ui.evaluate(() => { document.querySelector('#memory-app').scrollTop = 0 })
      if (process.env.CDR_BROWSER_SCREENSHOT) await ui.screenshot({ path: process.env.CDR_BROWSER_SCREENSHOT, fullPage: true })
    } catch (error) {
      const diagnostic = await ui.evaluate(() => window.memoryUiTest?.state()).catch(() => null)
      const compact = diagnostic ? { status: diagnostic.status, text: diagnostic.text, draft: diagnostic.draft,
        surfaceState: diagnostic.surfaceState, pending: diagnostic.pending, document: diagnostic.document, errors: diagnostic.errors,
        generation: diagnostic.repository?.generation, head: diagnostic.repository?.aggregate.session.head } : null
      throw new Error(phase + ': ' + error.message + '\nMemory UI state: ' + JSON.stringify(compact))
    } finally {
      await ui.close()
    }
  })

  assert.deepEqual(pageErrors, [], 'uncaught browser errors')
  assert.deepEqual(failedResponses, [], 'all browser entry and asset requests succeed')
  console.log(JSON.stringify({ browser: await browser.version(), results }, null, 2))
} finally {
  await restoreClipboard().catch((error) => { failures++; console.error('Clipboard restore failed: ' + error.message) })
  await browser?.close()
  await server.close()
}
if (failures) process.exitCode = 1
