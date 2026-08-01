<!-- App.svelte — Roam Research import UI, v2 plugin port of the host's
     src/roam-import-app.svelte. Runs inside a host plugin window with ZERO
     Tauri IPC: all host effects go through the window.notemd fetch-RPC bridge
     (see src/lib/bridge.ts). The parse/plan/convert core is unchanged. -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { setLocale, t } from './lib/strings'
  import {
    clipboardWrite, dialogOpenJson, toast, vaultInfo, bridge,
  } from './lib/bridge'
  import { sha256Hex } from './lib/hash'
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
          const conv = convertPage(f.page, graph.referencedUids, assigned.renames)
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
