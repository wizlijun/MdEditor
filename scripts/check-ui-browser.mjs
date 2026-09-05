#!/usr/bin/env node
// Real Svelte windows with isolated in-memory native/agent boundaries.
// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node scripts/check-ui-browser.mjs
// UI_REVIEW_FILTER=memory UI_REVIEW_OUTPUT=/absolute/path narrows/output artifacts.
import assert from 'node:assert/strict'
import { mkdtemp, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createServer } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const output = process.env.UI_REVIEW_OUTPUT ?? await mkdtemp(join(tmpdir(), 'notemd-ui-review-'))
await mkdir(output, { recursive: true })
const { chromium } = process.env.PLAYWRIGHT_MODULE
  ? await import(pathToFileURL(resolve(process.env.PLAYWRIGHT_MODULE)).href) : await import('playwright')
const apps = ['settings', 'vault-settings', 'market', 'agent-picker', 'claude-agent', 'codex-agent', 'deepseek-agent', 'openclaw', 'idea-spark', 'trace-source', 'next', 'decision-log', 'weekly-review', 'ebook-import', 'roam-import', 'power-mode', 'meetings', 'memory']
const selected = apps.filter((app) => !process.env.UI_REVIEW_FILTER || app.includes(process.env.UI_REVIEW_FILTER))
const server = await createServer({ root, resolve: { dedupe: ['svelte'] },
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false, watch: null },
  plugins: [{ name: 'ui-review-fixture',
    transform(code, id) {
      if (/\/plugins-src\/.*\/editor-kit(?:-v2)?\.ts$/.test(id)) {
        return code.replace(/return \x60plugin:[^\n]+\x60/, `return '${id.endsWith('editor-kit-v2.ts') ? '/src/editor-kit-v2/main.ts' : '/src/editor-kit/main.ts'}'`)
      }
      return null
    },
    configureServer(vite) {
      for (const route of ['/src/editor-kit/editor-kit-v1.css', '/src/editor-kit-v2/editor-kit-v1.css']) {
        vite.middlewares.use(route, (_req, res) => { res.setHeader('Content-Type', 'text/css'); res.end('') })
      }
      vite.middlewares.use('/__ui-review', async (req, res, next) => {
        if (req.url?.includes('html-proxy')) { next(); return }
        const kind = new URL(req.url, 'http://fixture').searchParams.get('app') ?? 'settings'
        if (!apps.includes(kind)) { res.statusCode = 404; res.end(); return }
        const module = kind === 'settings' ? '/src/components/SettingsDialog.svelte' : kind === 'vault-settings' ? '/src/components/VaultSettingsTab.svelte' : kind === 'agent-picker' ? '/src/lib/agent-picker/AgentPicker.svelte' : kind === 'market' ? '/src/plugin-market-app.svelte' : `/plugins-src/${kind}/src/App.svelte`
        const props = kind === 'settings' ? '{ open: true }' : kind === 'agent-picker' ? `{ options: [{id:'claude',name:'Claude',harness:{ok:true,harness:'Claude Code',version:'1.0'}},{id:'codex',name:'Codex',harness:{ok:true,harness:'Codex',version:'1.0'}}], selected:'claude', onselect:(id)=>window.__uiReview.selected=id, label:(key,vars)=>vars?.name ? '使用 '+vars.name : key }` : '{}'
        res.setHeader('Content-Type', 'text/html; charset=utf-8')
        res.end(await vite.transformIndexHtml('/__ui-review?app=' + kind, `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>html,body{margin:0;min-width:0;color-scheme:light dark;background:Canvas;color:CanvasText}#fixture{height:100vh}</style></head><body><div id="fixture" class="ui-surface"></div><script type="module">
          import { installUiReviewBridge } from '/scripts/fixtures/ui-review-bridge.js';
          import '/src/styles/ui-foundation.css'; import '/src/editor-kit/kit.css';
          import { mount, flushSync } from 'svelte';
          installUiReviewBridge(${JSON.stringify(kind)});
          try {
            const { default: App } = await import(${JSON.stringify(module)});
            const { loadLocale } = await import('/src/lib/i18n/store.svelte.ts'); await loadLocale();
            ${kind === 'openclaw' ? "const {setLocale} = await import('/plugins-src/openclaw/src/lib/strings.ts'); setLocale(window.notemd.locale);" : ''}
            window.__uiReview.component = mount(App, { target: document.querySelector('#fixture'), props: ${props} });
            flushSync(); window.__uiReview.ready = true;
          } catch (error) { window.__uiReview.error = String(error); document.body.append(String(error)); }
        </script></body></html>`))
      })
    },
  }],
})
let browser
const results = []
try {
  await server.listen()
  browser = await chromium.launch({ headless: process.env.UI_REVIEW_HEADED !== '1', ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) })
  const port = server.httpServer.address().port
  for (const app of selected) {
    const context = await browser.newContext({ viewport: { width: 1100, height: 760 }, colorScheme: 'light', reducedMotion: 'reduce' })
    const page = await context.newPage()
    const errors = []
    page.on('pageerror', (error) => errors.push(String(error)))
    try {
      await page.goto(`http://127.0.0.1:${port}/__ui-review?app=${app}`)
      await page.waitForFunction(() => window.__uiReview?.ready || window.__uiReview?.error)
      assert.equal(await page.evaluate(() => window.__uiReview.error), null)
      await page.waitForTimeout(700)
      const text = await page.locator('body').innerText()
      if (app === 'trace-source') {
        // An empty composer has only seven label characters; its CSS placeholder
        // does not contribute to innerText. Assert real controls, not copy length.
        await page.locator('.ProseMirror[contenteditable="true"]').waitFor()
        await page.getByRole('button', { name: '新溯源', exact: true }).waitFor()
        await page.getByRole('button', { name: '开始溯源', exact: true }).waitFor()
      } else assert.ok(text.trim().length > 0, 'window must render actual content')
      for (const variant of ['light', 'dark', 'narrow']) {
        await page.emulateMedia({ colorScheme: variant === 'light' ? 'light' : 'dark' })
        await page.setViewportSize(variant === 'narrow' ? { width: 390, height: 640 } : { width: 1100, height: 760 })
        await page.waitForTimeout(80)
        const overflow = await page.evaluate(() => ({ width: window.innerWidth, body: document.body.scrollWidth, html: document.documentElement.scrollWidth }))
        assert.ok(Math.max(overflow.body, overflow.html) <= overflow.width + 2, `${variant}: horizontal overflow ${JSON.stringify(overflow)}`)
        await page.screenshot({ path: join(output, `${app}-${variant}.png`) })
      }
      if (['claude-agent', 'codex-agent', 'deepseek-agent'].includes(app)) {
        await page.getByRole('button', { name: '设置', exact: true }).click()
        await page.getByRole('heading', { name: '设置', exact: true }).waitFor()
        await page.screenshot({ path: join(output, `${app}-settings.png`) })
        await page.keyboard.press('Tab')
        assert.ok(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle !== 'none'), 'keyboard focus is visible')
      }
      if (app === 'settings') {
        const navigation = page.locator('nav.tab-strip button')
        const count = await navigation.count()
        assert.ok(count >= 6, 'host settings navigation is populated')
        for (let index = 0; index < count; index++) {
          await navigation.nth(index).click()
          await page.waitForTimeout(100)
          assert.equal(await navigation.nth(index).getAttribute('aria-current'), 'page')
          assert.ok(await page.getByRole('dialog').evaluate((node) => node.scrollWidth <= node.clientWidth + 2), 'settings panel fits the narrow dialog')
          await page.screenshot({ path: join(output, `settings-section-${index + 1}.png`) })
        }
      }
      if (app === 'meetings') {
        await page.getByRole('button', { name: '设置', exact: true }).click()
        await page.getByLabel('会议逐字稿目录', { exact: true }).waitFor()
        await page.screenshot({ path: join(output, `${app}-settings.png`) })
      }
      if (app === 'agent-picker') {
        const trigger = page.getByRole('button', { name: '使用 Claude' })
        await trigger.focus()
        await page.keyboard.press('ArrowDown')
        const entries = page.getByRole('menuitemradio')
        await entries.first().waitFor()
        await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menuitemradio')
        assert.ok(await entries.first().evaluate((node) => node === document.activeElement), 'picker focuses the current provider')
        await entries.last().hover()
        const hover = await entries.last().evaluate((node) => ({ bg: getComputedStyle(node).backgroundColor, text: getComputedStyle(node).color }))
        assert.equal(hover.text, 'rgb(255, 255, 255)', 'menu hover uses white text')
        const rgb = hover.bg.match(/\d+/g)?.map(Number)
        assert.ok(rgb && rgb[2] > rgb[0] && rgb[2] > rgb[1], 'the global accent hover beats scoped button resets')
        await page.screenshot({ path: join(output, 'agent-picker-hover.png') })
        await page.keyboard.press('ArrowDown')
        await page.keyboard.press('Enter')
        assert.equal(await page.evaluate(() => window.__uiReview.selected), 'codex')
        assert.ok(await trigger.evaluate((node) => node === document.activeElement), 'picker returns focus after selection')
      }
      if (app === 'trace-source') {
        const editor = page.locator('.ProseMirror[contenteditable="true"]')
        await editor.fill('检查这段背景的原始来源。')
        assert.equal(await editor.innerText(), '检查这段背景的原始来源。')
        const settings = page.getByRole('button', { name: '设置', exact: true })
        await settings.click()
        const dialog = page.getByRole('dialog', { name: '设置', exact: true })
        await dialog.waitFor()
        await page.waitForFunction(() => document.activeElement?.id === 'trace-dir')
        await page.keyboard.press('Shift+Tab')
        assert.ok(await dialog.evaluate((node) => node.contains(document.activeElement)), 'settings traps reverse Tab')
        await page.keyboard.press('Escape')
        await dialog.waitFor({ state: 'detached' })
        assert.ok(await settings.evaluate((node) => node === document.activeElement), 'settings restores its trigger')
        await settings.click()
        await dialog.locator('#trace-dir').fill('inbox/reviewed-traces')
        await page.evaluate(() => { window.__uiReview.failTraceSettings = true })
        await dialog.getByRole('button', { name: '保存', exact: true }).click()
        await dialog.getByRole('alert').waitFor({ timeout: 2000 })
        assert.equal(await dialog.locator('#trace-dir').inputValue(), 'inbox/reviewed-traces', 'failed setting draft remains editable')
        await page.screenshot({ path: join(output, 'trace-source-settings-failure.png') })
        await page.evaluate(() => { window.__uiReview.failTraceSettings = false })
        await dialog.getByRole('button', { name: '保存', exact: true }).click()
        await dialog.waitFor({ state: 'detached' })
        assert.equal(await editor.innerText(), '检查这段背景的原始来源。', 'directory save preserves composer text')
      }
      if (app === 'idea-spark') {
        const settings = page.getByRole('button', { name: '设置', exact: true })
        await settings.click()
        const dialog = page.getByRole('dialog', { name: '设置', exact: true })
        await dialog.waitFor()
        await page.waitForFunction(() => document.activeElement?.id === 'idea-dir')
        await dialog.locator('#idea-dir').fill('inbox/reviewed-ideas')
        await page.evaluate(() => { window.__uiReview.failIdeaSettings = true })
        await dialog.getByRole('button', { name: '保存', exact: true }).click()
        await dialog.getByRole('alert').waitFor({ timeout: 2000 })
        assert.equal(await dialog.locator('#idea-dir').inputValue(), 'inbox/reviewed-ideas', 'failed idea settings draft remains editable')
        await page.screenshot({ path: join(output, 'idea-spark-settings-failure.png') })
        await page.evaluate(() => { window.__uiReview.failIdeaSettings = false })
        await dialog.getByRole('button', { name: '保存', exact: true }).click()
        await dialog.waitFor({ state: 'detached' })
        assert.ok(await settings.evaluate((node) => node === document.activeElement), 'idea settings restores trigger after save')
      }
      if (app === 'memory') {
        await page.getByRole('button', { name: '添加主张', exact: true }).click()
        const dialog = page.getByRole('dialog', { name: '添加主张', exact: true })
        await dialog.waitFor()
        await page.keyboard.press('Shift+Tab')
        assert.ok(await dialog.evaluate((node) => node.contains(document.activeElement)))
        await page.screenshot({ path: join(output, `${app}-add.png`) })
        await dialog.getByRole('textbox', { name: '主张内容', exact: true }).fill('明确记录背景，不扩大操作授权。')
        await page.evaluate(() => {
          const request = window.notemd.request
          window.notemd.request = (method, params) => method === 'host.memory.v2.add'
            ? new Promise((_resolve, reject) => { window.__uiReview.rejectAdd = () => reject(new Error('保存失败，请重试')) })
            : request(method, params)
        })
        await dialog.getByRole('button', { name: '保存并确认', exact: true }).click()
        assert.ok(await dialog.getByRole('textbox', { name: '主张内容', exact: true }).isDisabled(), 'all add fields are protected during persistence')
        await page.keyboard.press('Escape')
        assert.ok(await dialog.isVisible(), 'saving prevents dismissal')
        await page.evaluate(() => window.__uiReview.rejectAdd())
        await dialog.getByRole('alert').waitFor()
        assert.equal(await dialog.getByRole('textbox', { name: '主张内容', exact: true }).inputValue(), '明确记录背景，不扩大操作授权。')
        assert.ok(await dialog.getByRole('textbox', { name: '主张内容', exact: true }).isEnabled())
        await page.screenshot({ path: join(output, 'memory-save-failure.png') })
        await page.keyboard.press('Escape')
        await dialog.waitFor({ state: 'detached' })
      }
      assert.deepEqual(errors, [], 'no uncaught browser errors')
      results.push({ app, status: 'passed', controls: await page.locator('button,input,select,textarea').count() })
      console.log('PASS', app)
    } catch (error) {
      await page.screenshot({ path: join(output, `${app}-failure.png`) }).catch(() => {})
      results.push({ app, status: 'failed', error: String(error), errors })
      console.error('FAIL', app, error.stack ?? String(error), errors)
    } finally { await context.close() }
  }
  console.log(JSON.stringify({ browser: browser.version(), output, results }, null, 2))
  if (results.some((result) => result.status === 'failed')) process.exitCode = 1
} finally { await browser?.close(); await server.close() }
