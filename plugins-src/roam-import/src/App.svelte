<!-- App.svelte — Roam Research import UI, v2 plugin port of the host's
     src/roam-import-app.svelte. Runs inside a host plugin window with ZERO
     Tauri IPC: all host effects go through the window.notemd fetch-RPC bridge
     (see src/lib/bridge.ts). The parse/plan/convert core is unchanged. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { setLocale, t } from './lib/strings'
  import {
    clipboardWrite, dialogOpenJson, toast, vaultInfo, bridge,
    probe, syncDay, type RoamProbe, type SyncOutcome,
  } from './lib/bridge'
  import { sha256Hex } from './lib/hash'
  import { reconcileGraphChoice } from './lib/graph-choice'
  import { parseRoamJson } from './lib/roam-import/parse'
  import { assignFiles, planActions, type PlannedPage } from './lib/roam-import/plan'
  import { convertPage, type ConvertedPage } from './lib/roam-import/convert'
  import { pageKey, type ImportManifest } from './lib/roam-import/types'
  import {
    readRoamExport, writeNoteFile, localFileHash,
    loadImportManifest, saveImportManifest,
  } from './lib/roam-import/io'

  type LogEntry = { level: 'error' | 'warn'; page: string; message: string }
  type Stage = 'idle' | 'parse' | 'plan' | 'write' | 'done'

  let ready = $state(false)
  let stage = $state<Stage>('idle')
  let total = $state(0)
  let done = $state(0)
  let current = $state('')
  let log = $state<LogEntry[]>([])
  let summary = $state<{ wiki: number; daily: number; skipped: number } | null>(null)
  let conflicts = $state<Array<{ key: string; relPath: string; selected: boolean }>>([])
  /** 冲突覆盖重试所需的转换缓存(非响应式) */
  let convertedByKey: Map<string, { relPath: string; page: ConvertedPage }> = new Map()
  let manifestDraft: ImportManifest | null = null

  /** Vault context resolved once at startup via host.vault.info. */
  let vaultRoot = $state<string | null>(null)
  let wikiDir = 'wikipage'
  let dailyDir = 'dailynote'

  // ── Roam CLI daily sync (separate flow from the JSON-export import above;
  //    its state is kept apart on purpose, see Task 8 brief). useCli/cliDate/
  //    cliGraph are UI preferences only — they live in localStorage, never the
  //    backend. ──
  const CLI_TOGGLE_KEY = 'roam-import:cli:useCli'
  const CLI_DATE_KEY = 'roam-import:cli:date'
  const CLI_GRAPH_KEY = 'roam-import:cli:graph'

  /** YYYY-MM-DD for yesterday in the machine's local calendar (a daily note
   *  is a human's day, not UTC's — mirrors the backend's own default day). */
  function yesterdayLocal(): string {
    const d = new Date()
    d.setDate(d.getDate() - 1)
    const y = d.getFullYear()
    const m = String(d.getMonth() + 1).padStart(2, '0')
    const day = String(d.getDate()).padStart(2, '0')
    return `${y}-${m}-${day}`
  }

  function loadUseCli(): boolean {
    try { return localStorage.getItem(CLI_TOGGLE_KEY) === '1' } catch { return false }
  }
  function loadCliDate(): string {
    try { return localStorage.getItem(CLI_DATE_KEY) || yesterdayLocal() } catch { return yesterdayLocal() }
  }
  function loadCliGraph(): string {
    try { return localStorage.getItem(CLI_GRAPH_KEY) ?? '' } catch { return '' }
  }

  let useCli = $state(loadUseCli())
  let cliDate = $state(loadCliDate())
  /** Which Roam graph to read from. Empty = let the CLI auto-select, which it
   *  only does when exactly one graph is configured — so a two-graph user MUST
   *  have picked one here or `roam datalog-query` fails. */
  let cliGraph = $state(loadCliGraph())
  let probeResult = $state<RoamProbe | null>(null)
  let probeError = $state<string | null>(null)
  let syncing = $state(false)
  let syncResult = $state<SyncOutcome | null>(null)
  let syncError = $state<string | null>(null)

  $effect(() => {
    try { localStorage.setItem(CLI_TOGGLE_KEY, useCli ? '1' : '0') } catch { /* best-effort */ }
  })
  $effect(() => {
    try { localStorage.setItem(CLI_DATE_KEY, cliDate) } catch { /* best-effort */ }
  })
  $effect(() => {
    try { localStorage.setItem(CLI_GRAPH_KEY, cliGraph) } catch { /* best-effort */ }
  })

  /** Keep the persisted graph honest against what the CLI can actually see —
   *  but only where the probe actually says something: a name that is no
   *  longer configured would fail every sync, while a probe that reports no
   *  graphs at all (Roam not running) is not evidence about anything and must
   *  leave the pick where it is. The decision itself lives in
   *  lib/graph-choice.ts, where it is tested. */
  function reconcileGraph(p: RoamProbe) {
    cliGraph = reconcileGraphChoice(cliGraph, p.graphs)
  }

  async function refreshProbe() {
    probeError = null
    try {
      const p = await probe()
      reconcileGraph(p)
      probeResult = p
    } catch (e) {
      console.error('[roam-import] probe failed:', e)
      probeResult = null
      probeError = e instanceof Error ? e.message : String(e)
    }
  }

  /** The probe is not free: it spawns a full interactive login shell
   *  (`$SHELL -l -i -c 'command -v roam'`, which sources the user's rc files)
   *  plus up to two `roam` subprocesses. Someone who only ever uses the JSON
   *  import must not pay for that on every window open — so probe when the
   *  toggle is switched on, and on mount only if it is already on. */
  function setUseCli(on: boolean) {
    useCli = on
    if (on && probeResult === null) void refreshProbe()
  }

  async function runSync() {
    if (probeResult?.state !== 'ready' || syncing) return
    syncing = true
    syncError = null
    syncResult = null
    try {
      syncResult = await syncDay(cliDate, cliGraph ? { graph: cliGraph } : undefined)
    } catch (e) {
      syncError = e instanceof Error ? e.message : String(e)
    } finally {
      syncing = false
    }
  }

  onMount(async () => {
    try {
      setLocale(bridge().locale)
      const info = await vaultInfo()
      vaultRoot = info.root
      wikiDir = info.wiki_dir ?? 'wikipage'
      dailyDir = info.daily_dir ?? 'dailynote'
    } catch (e) {
      console.error('[roam-import] init failed:', e)
    }
    ready = true
    // Only when the user has actually turned the CLI sync on — see setUseCli.
    if (useCli) void refreshProbe()
  })

  const busy = $derived(stage === 'parse' || stage === 'plan' || stage === 'write')
  const errorCount = $derived(log.filter((l) => l.level === 'error').length)
  const yieldUi = () => new Promise((r) => setTimeout(r))

  async function pickAndImport() {
    const picked = await dialogOpenJson(t('title'), t('dialog.filter'))
    if (typeof picked !== 'string') return
    log = []; summary = null; conflicts = []; done = 0; total = 0; current = ''
    convertedByKey = new Map()
    if (!vaultRoot) return
    try {
      stage = 'parse'
      await yieldUi()
      const graph = parseRoamJson(await readRoamExport(picked))

      stage = 'plan'
      await yieldUi()
      const assigned = assignFiles(graph.pages, { wikipage: wikiDir, dailynote: dailyDir })
      for (const w of assigned.warnings) log = [...log, { level: 'warn', page: '', message: w }]
      const prevManifest = await loadImportManifest()
      const entries: Array<{ key: string; relPath: string; editTime: number }> = []
      for (const f of assigned.files) {
        try {
          const conv = convertPage(f.page, assigned.renames)
          const key = pageKey(f.page)
          convertedByKey.set(key, { relPath: f.relPath, page: conv })
          entries.push({ key, relPath: f.relPath, editTime: conv.editTime })
        } catch (e) {
          log = [...log, { level: 'error', page: f.page.title, message: String(e) }]
        }
      }
      const hashes = new Map<string, string | null>()
      for (const en of entries) hashes.set(en.relPath, await localFileHash(en.relPath))
      const actions = planActions(entries, prevManifest, hashes)

      stage = 'write'
      total = actions.length
      manifestDraft = {
        graphName: picked.split('/').pop() ?? 'roam',
        importedAt: new Date().toISOString(),
        pages: { ...(prevManifest?.pages ?? {}) },
      }
      let wiki = 0, daily = 0, skipped = 0
      for (const a of actions) {
        const conv = convertedByKey.get(a.key)
        if (!conv) { done++; continue }
        current = conv.page.title
        if (a.action === 'skip') { skipped++ }
        else if (a.action === 'conflict') {
          conflicts = [...conflicts, { key: a.key, relPath: a.relPath, selected: false }]
        } else {
          await writePage(a, conv.page)
          if (a.relPath.startsWith(dailyDir)) daily++; else wiki++
        }
        done++
        if (done % 20 === 0) await yieldUi()
      }
      await saveImportManifest(manifestDraft)
      summary = { wiki, daily, skipped }
      stage = 'done'
      if (errorCount > 0) {
        await toast('warn', t('doneErrors', { errors: errorCount }))
      } else {
        await toast('success', t('done', { wiki, daily, skipped }))
      }
    } catch (e) {
      log = [...log, { level: 'error', page: '', message: t('errParse', { error: String(e) }) }]
      stage = 'done'
      await toast('error', t('errParse', { error: String(e) }))
    }
  }

  async function writePage(a: Pick<PlannedPage, 'key' | 'relPath'>, conv: ConvertedPage) {
    try {
      await writeNoteFile(a.relPath, conv.text)
      manifestDraft!.pages[a.key] = { file: a.relPath, editTime: conv.editTime, contentHash: await sha256Hex(conv.text) }
    } catch (e) {
      log = [...log, { level: 'error', page: conv.title, message: t('errWrite', { page: a.relPath, error: String(e) }) }]
    }
  }

  async function overwriteSelected() {
    if (!vaultRoot || !manifestDraft) return
    for (const c of conflicts.filter((c) => c.selected)) {
      const conv = convertedByKey.get(c.key)
      if (conv) await writePage({ key: c.key, relPath: c.relPath }, conv.page)
    }
    conflicts = conflicts.filter((c) => !c.selected)
    await saveImportManifest(manifestDraft)
  }

  async function copyLog() {
    const text = log.map((l) => `[${l.level}] ${l.page ? l.page + ': ' : ''}${l.message}`).join('\n')
    await clipboardWrite(text)
  }
</script>

<main>
  {#if !ready}
    <p class="msg">…</p>
  {:else}
    <h1>{t('title')}</h1>
    {#if vaultRoot === null}
      <p class="msg">{t('noVault')}</p>
    {:else}
      <section class="cli">
        <div class="cli-head">
          <label class="cli-toggle">
            <!-- deliberately not `bind:checked`: the probe must be kicked off
                 *after* useCli has flipped, and only then (see setUseCli). -->
            <input
              type="checkbox"
              checked={useCli}
              onchange={(e) => setUseCli(e.currentTarget.checked)}
            />
            {t('cli.toggle')}
          </label>
          <a class="link" href="https://github.com/Roam-Research/roam-tools" target="_blank" rel="noopener">
            {t('cli.link')}
          </a>
        </div>

        {#if useCli}
          <div class="cli-body">
            {#if probeError}
              <p class="status err">✗ {t('cli.probeFailed', { error: probeError })}</p>
            {:else if probeResult === null}
              <p class="msg">…</p>
            {:else if probeResult.state === 'missing'}
              <p class="status err">✗ {t('cli.state.missing')}</p>
              <p class="hint-line">{t('cli.install')}</p>
            {:else if probeResult.state === 'not_connected'}
              <p class="status warn">{t('cli.state.notConnected', { version: probeResult.version ?? '' })}</p>
              <p class="hint-line">{t('cli.connect')}</p>
            {:else}
              <p class="status ok">
                ✓ {t('cli.state.ready', { version: probeResult.version ?? '', graph: probeResult.graphs.join(', ') })}
              </p>
            {/if}

            <div class="cli-row">
              <label class="date-label">
                {t('cli.date')}
                <input type="date" bind:value={cliDate} />
              </label>
              <!-- Only when there is a choice to make: with one graph the roam
                   CLI auto-selects, and a picker would be noise. -->
              {#if probeResult && probeResult.graphs.length > 1}
                <label class="date-label">
                  {t('cli.graph')}
                  <select bind:value={cliGraph}>
                    {#each probeResult.graphs as g (g)}
                      <option value={g}>{g}</option>
                    {/each}
                  </select>
                </label>
              {/if}
              <button class="sync" onclick={runSync} disabled={probeResult?.state !== 'ready' || syncing}>
                {syncing ? t('cli.syncing') : t('cli.sync')}
              </button>
            </div>

            {#if syncError}
              <p class="banner error-banner">{t('cli.failed', { error: syncError })}</p>
            {:else if syncResult}
              {#if !syncResult.found}
                <p class="banner">{t('cli.noPage', { date: syncResult.date })}</p>
              {:else}
                <p class="banner ok-banner">
                  {t('cli.result', { created: syncResult.created, updated: syncResult.updated, kept: syncResult.kept_local })}
                  {#if syncResult.roam_gone_kept > 0}
                    {' '}{t('cli.resultGoneKept', { count: syncResult.roam_gone_kept })}
                  {/if}
                </p>
              {/if}
            {/if}
          </div>
        {/if}
      </section>

      <section class="hint">
        <p class="hint-title">{t('hint.title')}</p>
        <ol>
          <li>{t('hint.step1')}</li>
          <li>{t('hint.step2')}</li>
        </ol>
      </section>

      <button class="pick" onclick={pickAndImport} disabled={busy}>
        {t('pickFile')}
      </button>

      {#if stage !== 'idle'}
        <section class="progress">
          {#if stage === 'parse'}<p>{t('stage.parse')}</p>{/if}
          {#if stage === 'plan'}<p>{t('stage.plan')}</p>{/if}
          {#if stage === 'write' || stage === 'done'}
            <p>{t('stage.write')}</p>
            <progress max={total} value={done}></progress>
            <p class="counter">{t('progress', { done, total, current })}</p>
          {/if}
        </section>
      {/if}

      {#if summary}
        {#if errorCount > 0}
          <p class="banner error-banner">{t('doneErrors', { errors: errorCount })}</p>
        {:else}
          <p class="banner ok-banner">{t('done', { wiki: summary.wiki, daily: summary.daily, skipped: summary.skipped })}</p>
        {/if}
      {/if}

      {#if conflicts.length > 0}
        <section class="conflicts">
          <p>{t('conflicts', { count: conflicts.length })}</p>
          {#each conflicts as c}
            <label><input type="checkbox" bind:checked={c.selected} /> {c.relPath}</label>
          {/each}
          <button onclick={overwriteSelected} disabled={!conflicts.some((c) => c.selected)}>
            {t('overwriteSelected')}
          </button>
        </section>
      {/if}

      {#if log.length > 0}
        <section class="error-log">
          <header>
            <h2>{t('errors')}</h2>
            <button onclick={copyLog}>{t('copyLog')}</button>
          </header>
          <ul>
            {#each log as l}
              <li class={l.level}>{l.page ? `${l.page}: ` : ''}{l.message}</li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
  {/if}
</main>

<style>
  /* Standalone webview: opt into both schemes so Canvas/CanvasText follow the
     OS appearance (mirrors the host's src/insights-app.svelte). */
  :global(:root) { color-scheme: light dark; }
  :global(body) { margin: 0; font-family: -apple-system, system-ui, sans-serif; background: Canvas; color: CanvasText; }
  main { height: 100vh; overflow: auto; padding: 14px 18px; box-sizing: border-box; max-width: 640px; margin: 0 auto; }
  h1 { font-size: 16px; margin: 0 0 12px; }
  .msg { color: color-mix(in srgb, CanvasText 55%, transparent); font-size: 13px; padding: 20px; }
  .hint {
    margin: 0 0 14px;
    padding: 10px 14px;
    border-radius: 6px;
    background: color-mix(in srgb, CanvasText 6%, transparent);
    font-size: 13px;
  }
  .hint-title { margin: 0 0 6px; font-weight: 600; }
  .hint ol { margin: 0; padding-left: 20px; }
  .hint li { margin: 2px 0; line-height: 1.45; }
  .cli {
    margin: 0 0 14px;
    padding: 10px 14px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, CanvasText 15%, transparent);
    font-size: 13px;
  }
  .cli-head { display: flex; align-items: center; gap: 10px; }
  .cli-toggle { display: flex; align-items: center; gap: 6px; }
  .link { color: color-mix(in srgb, CanvasText 60%, transparent); text-decoration: underline; font-size: 12px; }
  .cli-body { margin-top: 10px; display: flex; flex-direction: column; gap: 8px; }
  .status { margin: 0; }
  .status.ok { color: #34c759; }
  .status.warn { color: #ff9500; }
  .status.err { color: #ff3b30; }
  .hint-line { margin: 0; font-size: 12px; opacity: 0.75; }
  .cli-row { display: flex; align-items: center; gap: 10px; }
  .date-label { display: flex; align-items: center; gap: 6px; font-size: 12px; }
  button.sync { font-size: 13px; padding: 5px 12px; }
  .pick { font-size: 14px; padding: 6px 14px; }
  .progress { margin-top: 14px; font-size: 13px; }
  progress { width: 100%; }
  .counter { font-size: 12px; opacity: 0.75; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .banner { padding: 10px 12px; border-radius: 6px; font-weight: 600; font-size: 13px; }
  .ok-banner { background: color-mix(in srgb, #34c759 18%, transparent); }
  .error-banner { background: color-mix(in srgb, #ff3b30 22%, transparent); }
  .conflicts { margin-top: 12px; padding: 10px 12px; font-size: 13px;
    border: 1px solid color-mix(in srgb, #ff9500 55%, transparent); border-radius: 6px; }
  .conflicts label { display: block; font-size: 12px; padding: 2px 0; }
  .error-log { margin-top: 14px; border: 1px solid color-mix(in srgb, #ff3b30 55%, transparent); border-radius: 6px; }
  .error-log header { display: flex; justify-content: space-between; align-items: center; padding: 6px 10px;
    background: color-mix(in srgb, #ff3b30 14%, transparent); }
  .error-log h2 { font-size: 13px; margin: 0; }
  .error-log ul { list-style: none; margin: 0; padding: 6px 10px; max-height: 220px; overflow: auto;
    font-size: 12px; font-family: ui-monospace, monospace; }
  .error-log li.error { color: #ff3b30; }
  .error-log li.warn { color: #ff9500; }
</style>
