#!/usr/bin/env node
// Real plugin main.ts entry points; only native RPC/account responses are isolated.
import assert from 'node:assert/strict'
import { mkdir, mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createServer } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const output = process.env.AGENT_UI_OUTPUT ?? await mkdtemp(join(tmpdir(), 'notemd-agent-ui-qa-'))
await mkdir(output, { recursive: true })
const { chromium } = process.env.PLAYWRIGHT_MODULE
  ? await import(pathToFileURL(resolve(process.env.PLAYWRIGHT_MODULE)).href) : await import('playwright')
const apps = ['claude-agent', 'codex-agent', 'deepseek-agent', 'openclaw']

function installFixture(app) {
  const pushes = [], calls = []
  const settings = { maxConcurrency: '2', usageDisplay: 'tip' }
  let messageId = 0
  const qa = window.__agentQa = { pushes, calls, pauseSave: false, rejectSave: false, finishSave: null, failSend: false, ready: false }
  const emit = (data) => pushes.forEach((callback) => callback(data))
  const run = (id) => ({
    run_id: id, task: '项目背景与引用检查', trigger: 'window', started_at: '2026-09-05T02:20:00Z', ended_at: '2026-09-05T02:21:00Z',
    status: id === 'run-1' ? 'success' : 'error', result: '已检查项目背景，保留原始来源并标记需复核的断言。', stderr_tail: '',
    artifacts: ['projects/background-review.md'], harness: 'notemd.example-agent-with-a-long-name',
    usage: { input_tokens: 1200, output_tokens: 360, cache_read_tokens: 0, cache_write_tokens: 0, reasoning_tokens: 0, reported_total_tokens: 1560 },
  })
  let runs = [run('run-1'), run('run-2')]
  window.notemd = {
    pluginId: 'notemd.' + app, locale: 'zh', theme: 'system', onMessage: (callback) => pushes.push(callback),
    async request(method, params = {}) {
      calls.push({ method, params })
      if (method === 'host.settings.get') return { settings: { ...settings } }
      if (method === 'host.settings.set') {
        if (qa.pauseSave) await new Promise((done) => { qa.finishSave = done })
        if (qa.rejectSave) { qa.rejectSave = false; throw new Error('Fixture persistence unavailable') }
        settings[params.key] = params.value
        return {}
      }
      if (method === 'plugin.tasks.list') return { ready: true, tasks: [{ id: 'review', name: '检查项目背景', description: '保留叙事、核对引用并标记需要复核的事实。', running: false }] }
      if (method === 'plugin.history.list') return { runs }
      if (method === 'plugin.history.log') return { log: '读取 projects/context.md\n核对来源 projects/evidence/notes.md\n检查完成。' }
      if (method === 'plugin.history.clear') { runs = []; return {} }
      if (method === 'plugin.history.delete') { runs = runs.filter((run) => run.run_id !== params.run_id); return {} }
      if (method === 'plugin.context.get') return { tab: { path: 'projects/context.md', selection: '' } }
      if (method === 'plugin.harness-status') return { ok: true, harness: app.replace('-agent', ''), version: '1.0', origin: '/Applications/Agent/Resources/long-path-that-must-wrap-without-clipping/bin/agent' }
      if (method === 'plugin.connect') { queueMicrotask(() => emit({ kind: 'status', data: 'connected' })); return 'host' }
      if (method === 'plugin.send') {
        if (params.frame.type === 'session.list') {
          queueMicrotask(() => emit({ kind: 'frame', data: { type: 'session.list.result', sessions: [{ id: 'project-1', title: '项目文档评审 · 保留背景与来源' }], focus: 'project-1' } }))
        } else if (params.frame.type === 'user.message') {
          if (qa.failSend) { qa.failSend = false; throw new Error('连接暂时不可用，请重试。') }
          queueMicrotask(() => emit({ kind: 'frame', data: { type: 'agent.message.delta', session: 'project-1', msg_id: 'agent-' + ++messageId, text: '收到。我会保留原始背景与来源，逐段检查引用。\n请查看 [项目评审文档](projects/background-review.md)。' } }))
        }
        return {}
      }
      if (method === 'plugin.list_devices') return []
      return {}
    },
  }
}

const server = await createServer({
  root, resolve: { dedupe: ['svelte'] }, server: { host: '127.0.0.1', port: 5249, strictPort: false, hmr: false, watch: null },
  plugins: [{
    name: 'agent-ui-qa',
    configureServer(vite) {
      vite.middlewares.use('/__agent-qa', async (req, res, next) => {
        if (req.url?.includes('html-proxy')) { next(); return }
        const app = new URL(req.url, 'http://fixture').searchParams.get('app')
        if (!apps.includes(app)) { res.statusCode = 404; res.end(); return }
        const id = app === 'deepseek-agent' ? 'claude-agent-app' : app + '-app'
        res.setHeader('Content-Type', 'text/html; charset=utf-8')
        res.end(await vite.transformIndexHtml('/__agent-qa?app=' + app, `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><div id="${id}"></div><script type="module">
          (${installFixture.toString()})(${JSON.stringify(app)});
          await import('/plugins-src/${app}/src/main.ts'); window.__agentQa.ready = true;
        </script></body></html>`))
      })
    },
  }],
})
let browser
try {
  await server.listen()
  browser = await chromium.launch({ headless: true, ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) })
  const port = server.httpServer.address().port
  for (const app of apps) {
    const context = await browser.newContext({ viewport: app === 'openclaw' ? { width: 480, height: 720 } : { width: 980, height: 720 }, colorScheme: 'light', reducedMotion: 'reduce' })
    const page = await context.newPage()
    const errors = []
    page.on('pageerror', (error) => errors.push(String(error)))
    try {
      await page.goto(`http://127.0.0.1:${port}/__agent-qa?app=${app}`)
      await page.waitForFunction(() => window.__agentQa?.ready)
      if (app === 'openclaw') {
        await page.getByRole('status').filter({ hasText: '已连接' }).waitFor()
        assert.equal(await page.getByRole('combobox', { name: '当前会话' }).inputValue(), 'project-1')
        const composer = page.getByRole('textbox', { name: '输入发送给 OpenClaw…' })
        await composer.fill('请检查项目背景，保留知识的来源和适用条件。')
        await composer.press('Control+Enter')
        await page.locator('.bubble.agent').waitFor()
        await page.waitForFunction(() => document.activeElement === document.querySelector('textarea'))
        for (const mode of ['light', 'dark', 'narrow']) {
          await page.emulateMedia({ colorScheme: mode === 'light' ? 'light' : 'dark' })
          await page.setViewportSize(mode === 'narrow' ? { width: 360, height: 480 } : { width: 480, height: 720 })
          await page.screenshot({ path: join(output, `${app}-connected-${mode}.png`) })
          assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth))
        }
        await page.evaluate(() => { window.__agentQa.failSend = true })
        await composer.fill('失败后仍应保留的本地输入')
        await composer.press('Control+Enter')
        await page.getByRole('alert').filter({ hasText: '消息未发送' }).waitFor()
        assert.equal(await composer.inputValue(), '失败后仍应保留的本地输入')
        await page.waitForFunction(() => document.activeElement === document.querySelector('textarea'))
        await page.screenshot({ path: join(output, 'openclaw-send-failure.png') })
        await page.evaluate(() => window.__agentQa.pushes.forEach((push) => push({ kind: 'error', data: '连接暂时不可用' })))
        await page.getByRole('button', { name: '重试', exact: true }).click()
        await page.waitForFunction(() => !document.querySelector('.init-error'))
        assert.equal(await composer.inputValue(), '失败后仍应保留的本地输入')
        await page.emulateMedia({ forcedColors: 'active' })
        await page.screenshot({ path: join(output, 'openclaw-forced-colors.png') })
      } else {
        await page.locator('.row').first().waitFor()
        await page.setViewportSize({ width: 640, height: 420 })
        await page.locator('.row').first().click()
        await page.locator('.log').waitFor()
        await page.screenshot({ path: join(output, `${app}-history-min.png`) })
        await page.locator('.row').first().focus()
        await page.keyboard.press('Shift+F10')
        const menu = page.getByRole('menu')
        await menu.waitFor()
        await page.getByRole('menuitem', { name: '清空全部运行', exact: true }).hover()
        await page.screenshot({ path: join(output, `${app}-history-menu.png`) })
        assert.notEqual(await page.getByRole('menuitem').last().evaluate((node) => getComputedStyle(node).backgroundColor), 'rgba(0, 0, 0, 0)')
        await page.keyboard.press('Escape')
        await menu.waitFor({ state: 'detached' })
        assert.equal(await page.evaluate(() => document.activeElement?.classList.contains('row')), true)
        await page.getByRole('button', { name: '设置', exact: true }).click()
        const select = page.locator('#max-concurrency')
        await select.waitFor()
        await page.evaluate(() => { window.__agentQa.pauseSave = true })
        await select.focus()
        await select.selectOption('3')
        await page.getByRole('status').filter({ hasText: '正在保存' }).waitFor()
        await page.evaluate(() => { window.__agentQa.finishSave(); window.__agentQa.pauseSave = false })
        await page.getByRole('status').filter({ hasText: '已保存' }).waitFor()
        await page.waitForFunction(() => document.activeElement === document.querySelector('#max-concurrency'))
        assert.equal(await select.inputValue(), '3')
        await page.evaluate(() => { window.__agentQa.rejectSave = true })
        await select.selectOption('4')
        await page.getByRole('alert').filter({ hasText: '无法保存' }).waitFor()
        assert.equal(await select.inputValue(), '3')
        await page.screenshot({ path: join(output, `${app}-settings-save-failure.png`) })
      }
      assert.deepEqual(errors, [])
      console.log('PASS', app)
    } finally { await context.close() }
  }
  console.log(JSON.stringify({ output, browser: browser.version(), entries: apps, nativeAccounts: false }))
} finally { await browser?.close(); await server.close() }
