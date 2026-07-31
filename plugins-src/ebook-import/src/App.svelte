<script lang="ts">
  import { bridge } from './lib/bridge'
  import { setLocale, t, type MessageKey } from './lib/strings'
  import { addPaths, nextToStart, onJobEvent, type Queue, type QueueItem } from './lib/queue'

  setLocale(bridge().locale)

  // ── host push payload shapes (Task 1 drag-drop, Task 8 job events) ──────
  type JobPush = {
    type: 'job'
    job_id: number
    event: 'log' | 'progress' | 'done' | 'failed'
    line?: string
    stage?: string
    page?: number
    total?: number
    dest_rel?: string
    error?: string
  }
  type DragPush = { type: 'drag-drop'; phase: 'enter' | 'leave' | 'drop'; paths: string[] }
  type HostPush = JobPush | DragPush | { type: string }

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

  let q: Queue = $state({ items: [], activeId: null })
  let ocr = $state(false)
  let provider: 'wechat' | 'baidu' = $state('wechat')
  let dragActive = $state(false)
  let expanded: Record<number, boolean> = $state({})
  let globalError = $state('')

  let settingsOpen = $state(false)
  let calibreFound: { path: string; version: string } | null = $state(null)
  let ebooksRoot = $state('')
  let wechatUrl = $state('')
  let calibrePathOverride = $state('')
  let baiduKeyInput = $state('')
  let baiduSecretInput = $state('')
  let baiduKeySet = $state(false)
  let baiduSecretSet = $state(false)
  let saving = $state(false)
  let savedFlash = $state(false)

  function applyEnv(env: any) {
    calibreFound = env.calibre ?? null
    ebooksRoot = env.settings?.ebooks_root ?? ''
    wechatUrl = env.settings?.wechat_url ?? ''
    if (env.settings?.provider === 'baidu' || env.settings?.provider === 'wechat') {
      provider = env.settings.provider
    }
    calibrePathOverride = env.device?.calibre_path ?? ''
    baiduKeySet = !!env.device?.baidu_api_key_set
    baiduSecretSet = !!env.device?.baidu_secret_key_set
  }

  // The vault root resolves asynchronously on the backend (host.vault.info is
  // a round trip made from inside $activate) — detect_env answers
  // `ready:false` until that settles, so retry briefly instead of showing
  // "no calibre / no vault" while it's still in flight.
  async function loadEnv() {
    try {
      for (let i = 0; i < 20; i++) {
        const env = await bridge().request('plugin.detect_env', {})
        applyEnv(env)
        if (env.ready !== false) return
        await new Promise((r) => setTimeout(r, 500))
      }
    } catch (e) {
      globalError = message(e)
    }
  }

  /** Serial scheduler: starts the next pending item once nothing is active. */
  async function schedule() {
    const n = nextToStart(q)
    if (!n) return
    try {
      const res = await bridge().request('plugin.import_start', {
        path: n.path,
        ocr,
        ...(ocr ? { provider } : {}),
      })
      q = {
        ...q,
        activeId: n.id,
        items: q.items.map((i) => (i.id === n.id ? { ...i, status: 'running', jobId: res.job_id } : i)),
      }
    } catch (e) {
      q = {
        ...q,
        items: q.items.map((i) => (i.id === n.id ? { ...i, status: 'failed', error: message(e) } : i)),
      }
      void schedule() // this item never got an activeId — try the next one
    }
  }

  bridge().onMessage((raw: unknown) => {
    const m = raw as HostPush
    if (m.type === 'drag-drop') {
      const d = m as DragPush
      if (d.phase === 'enter') dragActive = true
      else if (d.phase === 'leave') dragActive = false
      else if (d.phase === 'drop') {
        dragActive = false
        q = addPaths(q, d.paths ?? [])
        void schedule()
      }
    } else if (m.type === 'job') {
      const j = m as JobPush
      q = onJobEvent(q, j.job_id, {
        event: j.event,
        line: j.line,
        stage: j.stage,
        page: j.page,
        total: j.total,
        dest_rel: j.dest_rel,
        error: j.error,
      })
      if (j.event === 'done' || j.event === 'failed') void schedule()
    }
  })

  async function pickFiles() {
    try {
      const res = await bridge().request('host.dialog.open', {
        multiple: true,
        filters: [{ name: 'Ebooks', extensions: ['epub', 'pdf', 'docx'] }],
      })
      const paths: string[] = res?.paths ?? []
      if (paths.length) {
        q = addPaths(q, paths)
        void schedule()
      }
    } catch (e) {
      globalError = message(e)
    }
  }

  async function pickCalibre() {
    try {
      const res = await bridge().request('host.dialog.open', { multiple: false, filters: [] })
      const p = res?.paths?.[0]
      if (p) calibrePathOverride = p
    } catch (e) {
      globalError = message(e)
    }
  }

  async function saveSettings() {
    saving = true
    globalError = ''
    try {
      await bridge().request('plugin.save_settings', {
        vault: { ebooks_root: ebooksRoot, wechat_url: wechatUrl },
        device: {
          calibre_path: calibrePathOverride,
          baidu_api_key: baiduKeyInput,
          baidu_secret_key: baiduSecretInput,
        },
      })
      // Secrets are never echoed back by detect_env — clear the plaintext
      // inputs so they don't linger on screen, then re-detect to refresh the
      // *_set flags and calibre status.
      baiduKeyInput = ''
      baiduSecretInput = ''
      await loadEnv()
      savedFlash = true
      setTimeout(() => (savedFlash = false), 1500)
    } catch (e) {
      globalError = message(e)
    } finally {
      saving = false
    }
  }

  async function cancelItem(item: QueueItem) {
    if (item.jobId == null) return
    try {
      await bridge().request('plugin.import_cancel', { job_id: item.jobId })
    } catch (e) {
      globalError = message(e)
    }
  }

  async function openInEditor(item: QueueItem) {
    if (!item.destRel) return
    try {
      const dir = item.destRel.replace(/\/+$/, '')
      await bridge().request('host.editor.open', { path: `${dir}/book.md` })
    } catch (e) {
      globalError = message(e)
    }
  }

  function clearFinished() {
    q = { ...q, items: q.items.filter((i) => i.status === 'pending' || i.status === 'running') }
  }

  function toggleLog(id: number) {
    expanded = { ...expanded, [id]: !expanded[id] }
  }

  function badgeKey(item: QueueItem): MessageKey {
    if (item.status === 'failed' && item.cancelled) return 'status.cancelled'
    return `status.${item.status}` as MessageKey
  }

  void loadEnv()
</script>

<!-- Drag highlighting is driven entirely by the host's `type:"drag-drop"`
     push (Task 1): this isolated webview's own OS-level drag-drop handling
     swallows native HTML5 dragenter/dragleave/drop before they reach the
     DOM, so there are deliberately no ondragenter/ondragover handlers here. -->
<main class:drag={dragActive}>
  <header>
    <h1>{t('title')}</h1>
    <button class="link" onclick={() => (settingsOpen = !settingsOpen)}>
      {t('settings.toggle')} {settingsOpen ? '▲' : '▼'}
    </button>
  </header>

  {#if globalError}
    <p class="error banner">{globalError}</p>
  {/if}

  {#if settingsOpen}
    <section class="settings">
      <label>
        {t('settings.root')}
        <input type="text" bind:value={ebooksRoot} />
      </label>
      <label>
        {t('settings.wechatUrl')}
        <input type="text" bind:value={wechatUrl} />
      </label>
      <label>
        {t('settings.baiduKey')}
        <input
          type="password"
          bind:value={baiduKeyInput}
          placeholder={baiduKeySet ? '••••••••' : ''}
        />
      </label>
      <label>
        {t('settings.baiduSecret')}
        <input
          type="password"
          bind:value={baiduSecretInput}
          placeholder={baiduSecretSet ? '••••••••' : ''}
        />
      </label>

      <div class="calibre-row">
        {#if calibreFound}
          <span class="ok">✓ {t('settings.calibre.found', { path: calibreFound.path, version: calibreFound.version })}</span>
        {:else}
          <span class="err">✗ {t('settings.calibre.missing')}</span>
          <a class="link" href="https://calibre-ebook.com" target="_blank" rel="noopener">
            {t('settings.calibre.install')}
          </a>
        {/if}
        <button class="secondary" onclick={pickCalibre}>{t('settings.calibre.pick')}</button>
      </div>

      <div class="save-row">
        <button class="primary" onclick={saveSettings} disabled={saving}>{t('settings.save')}</button>
        {#if savedFlash}<span class="ok">✓</span>{/if}
      </div>
    </section>
  {/if}

  <section class="dropzone">
    <p>{t('drop.hint')}</p>
    <button class="primary" onclick={pickFiles}>{t('drop.pick')}</button>
  </section>

  <section class="ocr">
    <label class="ocr-toggle">
      <input type="checkbox" bind:checked={ocr} />
      {t('ocr.label')}
    </label>
    {#if ocr}
      <select bind:value={provider}>
        <option value="wechat">{t('ocr.provider.wechat')}</option>
        <option value="baidu">{t('ocr.provider.baidu')}</option>
      </select>
    {/if}
    <span class="hint">{t('ocr.onlyPdf')}</span>
  </section>

  <section class="queue">
    <div class="queue-head">
      <button class="link" onclick={clearFinished}>{t('action.clear')}</button>
    </div>
    {#if q.items.length === 0}
      <p class="empty">{t('queue.empty')}</p>
    {:else}
      {#each q.items as item (item.id)}
        <div class="row">
          <div class="row-main">
            <button class="chevron" onclick={() => toggleLog(item.id)} aria-label={t('log.toggle')}>
              {expanded[item.id] ? '▾' : '▸'}
            </button>
            <span class="name" title={item.path}>{item.name}</span>
            <span class="badge {item.status}{item.cancelled ? ' cancelled' : ''}">
              {t(badgeKey(item))}
              {#if item.status === 'running' && item.total}
                {' '}{item.page ?? 0}/{item.total}
              {/if}
            </span>
            {#if item.status === 'running'}
              <button class="secondary" onclick={() => cancelItem(item)}>{t('action.cancel')}</button>
            {/if}
            {#if item.status === 'done'}
              <button class="link" onclick={() => openInEditor(item)}>{t('action.openInEditor')}</button>
            {/if}
          </div>
          {#if item.status === 'done' && item.destRel}
            <p class="dest">{item.destRel}</p>
          {/if}
          {#if item.status === 'failed' && !item.cancelled && item.error}
            <p class="error">{item.error}</p>
          {/if}
          {#if expanded[item.id]}
            <pre class="log">{item.logs.join('\n')}</pre>
          {/if}
        </div>
      {/each}
    {/if}
  </section>
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
  }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    font-size: 13px;
  }
  main {
    box-sizing: border-box;
    min-height: 100vh;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h1 {
    font-size: 15px;
    margin: 0;
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button.link {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.65;
    padding: 0;
  }
  button.link:hover {
    opacity: 1;
  }
  button.primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 6px;
    padding: 5px 14px;
    font-weight: 600;
    color: inherit;
  }
  button.secondary {
    background: transparent;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 6px;
    padding: 3px 10px;
    color: inherit;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }
  .settings label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 11px;
    opacity: 0.8;
  }
  .settings input {
    font: inherit;
    font-size: 13px;
    padding: 5px 7px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    background: transparent;
    color: inherit;
  }
  .calibre-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
  }
  .save-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
  .dropzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 22px;
    border: 1.5px dashed color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 10px;
    text-align: center;
    opacity: 0.85;
    transition: border-color 0.15s, background 0.15s;
  }
  main.drag .dropzone {
    border-color: color-mix(in srgb, currentColor 60%, transparent);
    background: color-mix(in srgb, currentColor 6%, transparent);
    opacity: 1;
  }
  .dropzone p {
    margin: 0;
    font-size: 12px;
    opacity: 0.7;
  }
  .ocr {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
  }
  .ocr-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ocr select {
    font: inherit;
    font-size: 12px;
    background: transparent;
    color: inherit;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 5px;
    padding: 3px 6px;
  }
  .ocr .hint {
    opacity: 0.5;
    font-size: 11px;
  }
  .queue {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    overflow-y: auto;
  }
  .queue-head {
    display: flex;
    justify-content: flex-end;
  }
  .empty {
    opacity: 0.5;
    font-size: 12px;
    text-align: center;
    padding: 16px 0;
  }
  .row {
    padding: 6px 4px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 10%, transparent);
  }
  .row-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .chevron {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.5;
    width: 14px;
    padding: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .badge {
    font-size: 10px;
    letter-spacing: 0.02em;
    padding: 2px 7px;
    border-radius: 10px;
    background: color-mix(in srgb, currentColor 10%, transparent);
    opacity: 0.85;
    flex: none;
  }
  .badge.done {
    color: #2e7d32;
  }
  .badge.failed {
    color: #c62828;
  }
  .badge.failed.cancelled {
    color: inherit;
    opacity: 0.6;
  }
  .badge.running {
    color: #1565c0;
  }
  .dest {
    margin: 2px 0 0 22px;
    font-size: 11px;
    opacity: 0.6;
  }
  p.error {
    margin: 2px 0 0 22px;
    font-size: 11px;
    color: #c62828;
  }
  p.error.banner {
    margin: 0;
    padding: 6px 10px;
    border-radius: 6px;
    background: color-mix(in srgb, #c62828 12%, transparent);
  }
  .log {
    margin: 4px 0 0 22px;
    padding: 6px 8px;
    max-height: 160px;
    overflow: auto;
    font-size: 11px;
    background: color-mix(in srgb, currentColor 6%, transparent);
    border-radius: 6px;
    white-space: pre-wrap;
  }
  .ok {
    color: #2e7d32;
  }
  .err {
    color: #c62828;
  }
</style>
