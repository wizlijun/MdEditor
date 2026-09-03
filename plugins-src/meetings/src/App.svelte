<script lang="ts">
  import { onMount } from 'svelte'
  import {
    bridge,
    cancelHemoryMigration,
    detectHemory,
    listMeetings,
    openInEditor,
    pickHemoryDirectory,
    planHemory,
    startHemoryMigration,
    toast,
    vaultInfo,
  } from './lib/bridge'
  import {
    REPORT_ACTIONS,
    actionTone,
    canPlan,
    hasProblems,
    initialUser,
    normalizeUsers,
    progressPercent,
    reportCount,
  } from './lib/migration'
  import { setLocale, t } from './lib/strings'
  import type {
    HemoryDetection,
    MeetingSummary,
    MigrationAction,
    MigrationMode,
    MigrationProgressPush,
    MigrationReport,
  } from './lib/types'

  type MigrationPhase = 'setup' | 'detecting' | 'planning' | 'preflight' | 'running' | 'result'

  let vaultReady = $state<boolean | null>(null)
  let meetings = $state<MeetingSummary[]>([])
  let libraryLoading = $state(true)
  let libraryError = $state('')
  let migrationOpen = $state(false)
  let phase = $state<MigrationPhase>('setup')
  let source = $state('')
  let detection: HemoryDetection | null = $state(null)
  let selectedUser = $state('')
  let timezone = $state('')
  let mode: MigrationMode = $state('incremental')
  let report: MigrationReport | null = $state(null)
  let flowError = $state('')
  let confirmOpen = $state(false)
  let runningJob: number | null = $state(null)
  let stopping = $state(false)
  let progressDone = $state(0)
  let progressTotal = $state(0)
  let currentItem = $state('')
  let detectGeneration = 0
  let planGeneration = 0
  const pendingPushes = new Map<number, MigrationProgressPush[]>()

  const users = $derived(normalizeUsers((detection as HemoryDetection | null) ?? { users: [] }))
  const needsTimezone = $derived(Boolean((detection as HemoryDetection | null)?.needs_timezone))
  const readyToPlan = $derived(canPlan(source, selectedUser, users, needsTimezone, timezone))
  const plannedWrites = $derived(
    ((report as MigrationReport | null)?.create ?? 0)
      + ((report as MigrationReport | null)?.update ?? 0),
  )
  const sortedMeetings = $derived([...meetings].sort((left, right) => (
    Date.parse(right.started_at) - Date.parse(left.started_at)
  )))

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error)
  }

  function formatDate(value: string): string {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
  }

  function formatDuration(ms?: number | null): string {
    if (typeof ms !== 'number' || ms < 0) return '—'
    const totalMinutes = Math.round(ms / 60_000)
    const hours = Math.floor(totalMinutes / 60)
    const minutes = totalMinutes % 60
    return hours ? `${hours}h ${minutes}m` : `${minutes}m`
  }

  function meetingSource(meeting: MeetingSummary): string {
    return meeting.source_system || meeting.source || 'local'
  }

  function transcriptPath(meeting: MeetingSummary): string | null {
    if (meeting.transcript_path) return meeting.transcript_path
    if (meeting.transcript_relative_path) return meeting.transcript_relative_path
    if (meeting.path) return meeting.path
    if (!meeting.target_relative_path) return null
    return `${meeting.target_relative_path}/transcript.srt`
  }

  async function refreshLibrary(): Promise<void> {
    libraryLoading = true
    libraryError = ''
    try {
      meetings = await listMeetings()
    } catch (error) {
      libraryError = `${t('error.library')} ${errorMessage(error)}`
    } finally {
      libraryLoading = false
    }
  }

  function resetMigration(): void {
    detectGeneration += 1
    planGeneration += 1
    phase = 'setup'
    source = ''
    detection = null
    selectedUser = ''
    timezone = ''
    mode = 'incremental'
    report = null
    flowError = ''
    confirmOpen = false
    runningJob = null
    stopping = false
    progressDone = 0
    progressTotal = 0
    currentItem = ''
  }

  function openMigration(): void {
    resetMigration()
    migrationOpen = true
  }

  function closeMigration(): void {
    if (phase === 'running') return
    migrationOpen = false
    resetMigration()
  }

  async function chooseSource(): Promise<void> {
    flowError = ''
    const picked = await pickHemoryDirectory(t('action.choose')).catch((error) => {
      flowError = errorMessage(error)
      return null
    })
    if (!picked) return

    const generation = ++detectGeneration
    planGeneration += 1
    source = picked
    detection = null
    selectedUser = ''
    timezone = ''
    report = null
    phase = 'detecting'
    try {
      const next = await detectHemory(picked)
      if (generation !== detectGeneration) return
      detection = next
      selectedUser = initialUser(next)
      timezone = next.timezone ?? ''
      phase = 'setup'
      if (canPlan(source, selectedUser, normalizeUsers(next), Boolean(next.needs_timezone), timezone)) {
        await runPreflight()
      }
    } catch (error) {
      if (generation !== detectGeneration) return
      phase = 'setup'
      flowError = `${t('error.detect')} ${errorMessage(error)}`
    }
  }

  function migrationOptions() {
    return {
      source,
      mode,
      ...(selectedUser ? { user: selectedUser } : {}),
      ...(timezone.trim() ? { timezone: timezone.trim() } : {}),
    }
  }

  async function runPreflight(): Promise<void> {
    if (!readyToPlan) return
    const generation = ++planGeneration
    flowError = ''
    report = null
    phase = 'planning'
    try {
      const next = await planHemory(migrationOptions())
      if (generation !== planGeneration) return
      report = next
      phase = 'preflight'
    } catch (error) {
      if (generation !== planGeneration) return
      phase = 'setup'
      flowError = `${t('error.plan')} ${errorMessage(error)}`
    }
  }

  function changeUser(event: Event): void {
    selectedUser = (event.currentTarget as HTMLSelectElement).value
    report = null
    phase = 'setup'
    if (selectedUser && !needsTimezone) void runPreflight()
  }

  function changeMode(next: MigrationMode): void {
    if (mode === next) return
    mode = next
    report = null
    phase = 'setup'
    if (readyToPlan) void runPreflight()
  }

  function changeTimezone(event: Event): void {
    timezone = (event.currentTarget as HTMLInputElement).value
    report = null
    phase = 'setup'
  }

  function applyPush(push: MigrationProgressPush): void {
    if (push.event === 'progress') {
      progressDone = push.committed ?? progressDone
      progressTotal = push.total ?? progressTotal
      currentItem = push.item?.conversation_id ?? currentItem
      return
    }
    stopping = false
    runningJob = null
    currentItem = ''
    if (push.report) report = push.report
    phase = 'result'
    if (push.event === 'done' || push.event === 'cancelled') {
      void refreshLibrary()
      if (push.event === 'done' && report) {
        void toast(hasProblems(report) ? 'warn' : 'success', hasProblems(report) ? t('migration.resultProblems') : t('migration.resultOk'))
      }
    } else {
      flowError = `${t('error.apply')} ${push.error ?? ''}`.trim()
      void refreshLibrary()
    }
  }

  function onMigrationPush(push: MigrationProgressPush): void {
    if (runningJob === push.job_id) {
      applyPush(push)
      return
    }
    const queued = pendingPushes.get(push.job_id) ?? []
    queued.push(push)
    pendingPushes.set(push.job_id, queued)
  }

  async function startMigration(): Promise<void> {
    if (!report || plannedWrites === 0) return
    confirmOpen = false
    phase = 'running'
    flowError = ''
    progressDone = 0
    progressTotal = plannedWrites
    currentItem = ''
    try {
      const started = await startHemoryMigration({
        ...migrationOptions(),
        expected_plan: report,
      })
      runningJob = started.job_id
      for (const push of pendingPushes.get(started.job_id) ?? []) applyPush(push)
      pendingPushes.delete(started.job_id)
    } catch (error) {
      runningJob = null
      phase = 'result'
      flowError = `${t('error.apply')} ${errorMessage(error)}`
    }
  }

  async function stopMigration(): Promise<void> {
    if (runningJob === null || stopping) return
    stopping = true
    try {
      await cancelHemoryMigration(runningJob)
    } catch (error) {
      stopping = false
      flowError = errorMessage(error)
    }
  }

  async function openMeeting(meeting: MeetingSummary): Promise<void> {
    const path = transcriptPath(meeting)
    if (!path) return
    try {
      await openInEditor(path)
    } catch (error) {
      libraryError = errorMessage(error)
    }
  }

  onMount(() => {
    setLocale(bridge().locale)
    bridge().onMessage((raw: unknown) => {
      const push = raw as Partial<MigrationProgressPush>
      if (push.type === 'hemory-migration' && typeof push.job_id === 'number' && push.event) {
        onMigrationPush(push as MigrationProgressPush)
      }
    })
    void vaultInfo()
      .then((info) => {
        vaultReady = Boolean(info.root)
        if (vaultReady) void refreshLibrary()
        else libraryLoading = false
      })
      .catch(() => {
        vaultReady = false
        libraryLoading = false
      })
  })
</script>

<main>
  <header class="topbar">
    <div>
      <h1>{t('title')}</h1>
      <p>{t('subtitle')}</p>
    </div>
    {#if !migrationOpen}
      <div class="header-actions">
        <button class="secondary" onclick={refreshLibrary} disabled={!vaultReady || libraryLoading}>{t('action.refresh')}</button>
        <button class="primary" onclick={openMigration} disabled={!vaultReady}>{t('action.migrate')}</button>
      </div>
    {:else if phase !== 'running'}
      <button class="secondary" onclick={closeMigration}>{t('action.close')}</button>
    {/if}
  </header>

  {#if vaultReady === false}
    <section class="empty-state"><p>{t('error.noVault')}</p></section>
  {:else if migrationOpen}
    <section class="migration-shell">
      <div class="migration-heading">
        <button class="back-link" onclick={closeMigration} disabled={phase === 'running'}>‹ {t('action.back')}</button>
        <h2>{t('migration.title')}</h2>
        <p>{t('migration.subtitle')}</p>
      </div>

      {#if flowError}<div class="banner error" role="alert">{flowError}</div>{/if}

      {#if phase === 'running'}
        <section class="running-card" aria-live="polite">
          <div class="running-icon"><span></span></div>
          <h3>{stopping ? t('migration.stopping') : t('migration.running')}</h3>
          <p>{t('migration.progress', { done: progressDone, total: progressTotal })}</p>
          <div class="progress-track" role="progressbar" aria-valuenow={progressDone} aria-valuemin="0" aria-valuemax={progressTotal}>
            <span style={`width: ${progressPercent(progressDone, progressTotal)}%`}></span>
          </div>
          {#if currentItem}<code>{currentItem}</code>{/if}
          <button class="secondary" onclick={stopMigration} disabled={stopping}>{t('action.cancel')}</button>
        </section>
      {:else}
        <section class="setup-card">
          <div class="field-block">
            <div class="field-copy">
              <div class="field-label">{t('migration.source')}</div>
              <p>{t('migration.sourceHint')}</p>
            </div>
            <button class="secondary" onclick={chooseSource} disabled={phase === 'detecting' || phase === 'planning'}>
              {source ? t('action.change') : t('action.choose')}
            </button>
          </div>
          {#if source}<div class="selected-path"><span aria-hidden="true">⌁</span><code>{source}</code></div>{/if}
          {#if phase === 'detecting'}<p class="busy-line"><span class="spinner"></span>{t('migration.detecting')}</p>{/if}

          {#if detection}
            {#if users.length > 1}
              <div class="field-block compact">
                <div class="field-copy"><label for="hemory-user">{t('migration.user')}</label></div>
                <select id="hemory-user" value={selectedUser} onchange={changeUser}>
                  <option value="">{t('migration.chooseUser')}</option>
                  {#each users as user (user.id)}<option value={user.id}>{user.label}</option>{/each}
                </select>
              </div>
            {:else if users.length === 1}
              <div class="detected-row"><span>{t('migration.user')}</span><strong>{users[0].label}</strong></div>
            {/if}

            {#if needsTimezone}
              <div class="timezone-field">
                <label for="hemory-timezone">{t('migration.timezone')}</label>
                <input id="hemory-timezone" type="text" value={timezone} oninput={changeTimezone} placeholder="Asia/Taipei" autocomplete="off" spellcheck="false" />
                <p>{t('migration.timezoneHint')}</p>
              </div>
            {/if}

            <fieldset class="mode-field">
              <legend>{t('migration.mode')}</legend>
              <label class:chosen={mode === 'incremental'}>
                <input type="radio" name="mode" checked={mode === 'incremental'} onchange={() => changeMode('incremental')} />
                <span><strong>{t('migration.incremental')}</strong><small>{t('migration.incrementalHint')}</small></span>
              </label>
              <label class:chosen={mode === 'full'}>
                <input type="radio" name="mode" checked={mode === 'full'} onchange={() => changeMode('full')} />
                <span><strong>{t('migration.full')}</strong><small>{t('migration.fullHint')}</small></span>
              </label>
            </fieldset>

            {#if detection.warnings?.length}
              <details class="warnings">
                <summary>{t('migration.detectWarning')} ({detection.warnings.length})</summary>
                <ul>{#each detection.warnings as warning}<li>{warning}</li>{/each}</ul>
              </details>
            {/if}

            {#if phase === 'planning'}
              <p class="busy-line"><span class="spinner"></span>{t('migration.planning')}</p>
            {:else if phase === 'setup'}
              <div class="setup-actions">
                <button class="primary" onclick={runPreflight} disabled={!readyToPlan}>{t('action.preflight')}</button>
              </div>
            {/if}
          {/if}
        </section>

        {#if report}
          <section class="report-card">
            <div class="report-heading">
              <div><h3>{phase === 'result' ? t('migration.result') : t('migration.preflight')}</h3><p>{phase === 'result' ? (hasProblems(report) ? t('migration.resultProblems') : t('migration.resultOk')) : t('migration.preflightHint')}</p></div>
              <span class="mode-badge">{report.mode === 'full' ? t('migration.full') : t('migration.incremental')}</span>
            </div>

            <div class="summary-grid">
              <div><strong>{report.scanned}</strong><span>{t('report.scanned')}</span></div>
              <div><strong>{report.eligible}</strong><span>{t('report.eligible')}</span></div>
              {#each REPORT_ACTIONS as action}
                {@const count = reportCount(report, action)}
                {#if count > 0}<div class={actionTone(action)}><strong>{count}</strong><span>{t(`report.${action}`)}</span></div>{/if}
              {/each}
              {#if phase === 'result'}<div class="positive"><strong>{report.committed}</strong><span>{t('report.committed')}</span></div>{/if}
            </div>

            {#if report.warnings.length}<div class="banner warning"><ul>{#each report.warnings as warning}<li>{warning}</li>{/each}</ul></div>{/if}
            {#if report.errors.length}<div class="banner error"><ul>{#each report.errors as error}<li>{error}</li>{/each}</ul></div>{/if}

            {#if report.items.length}
              <div class="item-list" aria-label={t('report.details')}>
                {#each report.items as item (`${item.conversation_id}:${item.source_relative_path}`)}
                  <details class="item-row">
                    <summary>
                      <span class={`action-dot ${actionTone(item.action)}`}></span>
                      <strong>{item.conversation_id}</strong>
                      <span class={`action-label ${actionTone(item.action)}`}>{t(`report.${item.action}`)}</span>
                      <span class="item-path">{item.source_relative_path}</span>
                    </summary>
                    <dl>
                      <div><dt>{t('report.sourcePath')}</dt><dd><code>{item.source_relative_path}</code></dd></div>
                      {#if item.target_relative_path}<div><dt>{t('report.targetPath')}</dt><dd><code>{item.target_relative_path}</code></dd></div>{/if}
                      {#if item.selected_transcript}<div><dt>{t('report.transcript')}</dt><dd><code>{item.selected_transcript}</code></dd></div>{/if}
                      {#if item.reason}<div><dt>Reason</dt><dd>{item.reason}</dd></div>{/if}
                    </dl>
                  </details>
                {/each}
              </div>
            {/if}

            <div class="report-actions">
              {#if phase === 'preflight'}
                {#if plannedWrites > 0}<button class="primary" onclick={() => (confirmOpen = true)}>{t('action.confirm')}</button>{:else}<p>{t('migration.noChanges')}</p>{/if}
              {:else}
                <button class="primary" onclick={closeMigration}>{t('action.done')}</button>
                <button class="secondary" onclick={runPreflight}>{t('action.retry')}</button>
              {/if}
            </div>
          </section>
        {/if}
      {/if}
    </section>
  {:else}
    <section class="library" aria-live="polite">
      {#if libraryLoading}
        <div class="empty-state"><span class="spinner"></span><p>{t('library.loading')}</p></div>
      {:else if libraryError}
        <div class="empty-state error-text"><p>{libraryError}</p><button class="secondary" onclick={refreshLibrary}>{t('action.retry')}</button></div>
      {:else if meetings.length === 0}
        <div class="empty-state"><div class="empty-icon" aria-hidden="true">⌁</div><h2>{t('library.empty')}</h2><p>{t('library.emptyHint')}</p><button class="primary" onclick={openMigration}>{t('action.migrate')}</button></div>
      {:else}
        <div class="meeting-list">
          {#each sortedMeetings as meeting (meeting.conversation_id)}
            <article class="meeting-row">
              <button class="meeting-main" onclick={() => openMeeting(meeting)} disabled={!transcriptPath(meeting)}>
                <span class="meeting-date">{formatDate(meeting.started_at)}</span>
                <strong>{meeting.title || meeting.conversation_id}</strong>
                <span class="meeting-meta">
                  <span>{formatDuration(meeting.duration_ms)}</span>
                  {#if meeting.speaker_count != null}<span>{t('library.speakers', { count: meeting.speaker_count })}</span>{/if}
                  <span>{t('library.source', { source: meetingSource(meeting) })}</span>
                </span>
              </button>
              <button class="open-button" onclick={() => openMeeting(meeting)} disabled={!transcriptPath(meeting)}>{t('action.open')} ›</button>
            </article>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#if confirmOpen && report}
    <div class="sheet-backdrop" role="presentation" onclick={(event) => event.target === event.currentTarget && (confirmOpen = false)}>
      <div class="confirm-sheet" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
        <div class="confirm-mark" aria-hidden="true">⇢</div>
        <h2 id="confirm-title">{t('migration.confirmTitle')}</h2>
        <p>{t('migration.confirmBody')}</p>
        <div class="confirm-counts"><span>{t('report.create')} <strong>{report.create}</strong></span><span>{t('report.update')} <strong>{report.update}</strong></span></div>
        <div class="confirm-actions"><button class="secondary" onclick={() => (confirmOpen = false)}>{t('action.close')}</button><button class="primary" onclick={startMigration}>{t('action.confirm')}</button></div>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(*) { box-sizing: border-box; }
  :global(html) { background: transparent; color-scheme: light dark; }
  :global(body) { --topbar-border: rgba(0,0,0,.1); --topbar-background: rgba(250,250,252,.86); margin: 0; min-width: 560px; background: #f5f5f7; color: #1d1d1f; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif; }
  :global(button), :global(input), :global(select) { font: inherit; }
  button { cursor: default; }
  button:disabled { cursor: default; opacity: .45; }
  main { min-height: 100vh; }
  .topbar { position: sticky; top: 0; z-index: 4; display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 22px 28px 18px; border-bottom: 1px solid var(--topbar-border); background: var(--topbar-background); backdrop-filter: blur(22px) saturate(160%); }
  h1, h2, h3, p { margin-top: 0; }
  .topbar h1 { margin: 0; font-size: 26px; letter-spacing: -.035em; }
  .topbar p { margin: 4px 0 0; color: #6e6e73; font-size: 13px; }
  .header-actions, .report-actions, .confirm-actions, .setup-actions { display: flex; align-items: center; gap: 10px; }
  button.primary, button.secondary { min-height: 34px; padding: 7px 14px; border-radius: 9px; border: 1px solid transparent; font-weight: 600; }
  button.primary { color: #fff; background: #007aff; }
  button.primary:hover:not(:disabled) { background: #006ee6; }
  button.secondary { color: inherit; border-color: rgba(0,0,0,.16); background: rgba(255,255,255,.72); }
  button.secondary:hover:not(:disabled) { background: #fff; }
  .library, .migration-shell { max-width: 1000px; margin: 0 auto; padding: 24px 28px 44px; }
  .empty-state { min-height: 430px; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; color: #6e6e73; }
  .empty-state h2 { margin: 8px 0 6px; color: #1d1d1f; font-size: 18px; }
  .empty-state p { max-width: 480px; line-height: 1.5; }
  .empty-icon { display: grid; place-items: center; width: 52px; height: 52px; border-radius: 15px; background: #e8e8ed; color: #007aff; font-size: 28px; }
  .meeting-list { display: grid; gap: 10px; }
  .meeting-row { display: flex; align-items: center; gap: 12px; padding: 5px 8px 5px 5px; border: 1px solid rgba(0,0,0,.1); border-radius: 13px; background: rgba(255,255,255,.82); box-shadow: 0 1px 2px rgba(0,0,0,.025); }
  .meeting-row:hover { border-color: rgba(0,122,255,.28); }
  .meeting-main { min-width: 0; flex: 1; display: grid; grid-template-columns: 160px minmax(180px, 1fr) auto; align-items: center; gap: 16px; padding: 13px; border: 0; text-align: left; color: inherit; background: transparent; }
  .meeting-main strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meeting-date { color: #6e6e73; font-size: 13px; }
  .meeting-meta { display: flex; gap: 12px; color: #6e6e73; font-size: 12px; white-space: nowrap; }
  .open-button, .back-link { border: 0; color: #007aff; background: transparent; }
  .open-button { padding: 8px; font-size: 13px; }
  .back-link { padding: 0; margin-bottom: 14px; }
  .migration-heading h2 { margin-bottom: 5px; font-size: 22px; }
  .migration-heading p { color: #6e6e73; font-size: 13px; }
  .setup-card, .report-card, .running-card { margin-top: 18px; padding: 20px; border: 1px solid rgba(0,0,0,.1); border-radius: 16px; background: rgba(255,255,255,.84); box-shadow: 0 3px 18px rgba(0,0,0,.04); }
  .field-block { display: flex; align-items: center; justify-content: space-between; gap: 20px; }
  .field-block.compact { padding: 15px 0; border-top: 1px solid rgba(0,0,0,.08); }
  .field-copy label, .field-label, .timezone-field label, legend { display: block; margin-bottom: 4px; font-weight: 650; font-size: 14px; }
  .field-copy p, .timezone-field p { margin: 0; color: #6e6e73; font-size: 12px; line-height: 1.4; }
  .selected-path { display: flex; align-items: center; gap: 8px; min-width: 0; margin-top: 12px; padding: 9px 11px; border-radius: 8px; background: rgba(0,0,0,.045); }
  code { font-family: "SF Mono", ui-monospace, monospace; font-size: 11px; overflow-wrap: anywhere; }
  select, input[type="text"] { min-height: 34px; padding: 6px 9px; border: 1px solid rgba(0,0,0,.18); border-radius: 8px; color: inherit; background: rgba(255,255,255,.82); }
  select { min-width: 210px; }
  .timezone-field { margin-top: 15px; padding-top: 15px; border-top: 1px solid rgba(0,0,0,.08); }
  .timezone-field input { width: min(100%, 360px); margin: 5px 0; }
  .mode-field { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin: 18px 0 0; padding: 15px 0 0; border: 0; border-top: 1px solid rgba(0,0,0,.08); }
  .mode-field legend { float: left; width: 100%; margin-bottom: 10px; }
  .mode-field > label { display: flex; align-items: flex-start; gap: 9px; padding: 12px; border: 1px solid rgba(0,0,0,.12); border-radius: 10px; }
  .mode-field > label.chosen { border-color: #007aff; box-shadow: 0 0 0 1px #007aff inset; background: rgba(0,122,255,.055); }
  .mode-field input { margin-top: 2px; accent-color: #007aff; }
  .mode-field span { display: grid; gap: 3px; }
  .mode-field strong { font-size: 13px; }
  .mode-field small { color: #6e6e73; line-height: 1.35; }
  .busy-line { display: flex; align-items: center; justify-content: center; gap: 8px; margin: 18px 0 0; color: #6e6e73; font-size: 13px; }
  .spinner { display: inline-block; width: 15px; height: 15px; border: 2px solid rgba(110,110,115,.25); border-top-color: #007aff; border-radius: 50%; animation: spin .75s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .detected-row { display: flex; justify-content: space-between; margin-top: 14px; padding: 12px 0; border-top: 1px solid rgba(0,0,0,.08); font-size: 13px; }
  .warnings { margin-top: 14px; color: #8a6300; font-size: 12px; }
  .warnings summary, .item-row summary { cursor: default; }
  .warnings li, .banner li { margin: 4px 0; }
  .setup-actions { justify-content: flex-end; margin-top: 16px; }
  .banner { margin: 15px 0 0; padding: 10px 12px; border-radius: 9px; font-size: 12px; line-height: 1.4; }
  .banner ul { margin: 0; padding-left: 18px; }
  .banner.error { color: #8f1616; background: rgba(255,59,48,.1); }
  .banner.warning { color: #735600; background: rgba(255,204,0,.13); }
  .report-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .report-heading h3 { margin-bottom: 4px; }
  .report-heading p { margin: 0; color: #6e6e73; font-size: 12px; }
  .mode-badge { padding: 4px 8px; border-radius: 999px; color: #5e5e63; background: rgba(0,0,0,.055); font-size: 11px; white-space: nowrap; }
  .summary-grid { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; }
  .summary-grid > div { min-width: 84px; display: grid; gap: 2px; padding: 9px 10px; border-radius: 9px; background: rgba(0,0,0,.045); }
  .summary-grid strong { font-size: 18px; }
  .summary-grid span { color: #6e6e73; font-size: 11px; }
  .summary-grid .positive strong, .action-label.positive { color: #16833b; }
  .summary-grid .warning strong, .action-label.warning { color: #9a6d00; }
  .summary-grid .danger strong, .action-label.danger { color: #d1261c; }
  .item-list { max-height: 300px; overflow: auto; margin-top: 16px; border: 1px solid rgba(0,0,0,.09); border-radius: 10px; }
  .item-row { border-bottom: 1px solid rgba(0,0,0,.075); }
  .item-row:last-child { border-bottom: 0; }
  .item-row summary { display: grid; grid-template-columns: 8px max-content max-content minmax(0,1fr); align-items: center; gap: 9px; padding: 10px 12px; list-style: none; font-size: 12px; }
  .item-row summary::-webkit-details-marker { display: none; }
  .action-dot { width: 7px; height: 7px; border-radius: 50%; background: #8e8e93; }
  .action-dot.positive { background: #34c759; } .action-dot.warning { background: #ff9f0a; } .action-dot.danger { background: #ff3b30; }
  .action-label { font-weight: 650; }
  .item-path { overflow: hidden; color: #6e6e73; text-overflow: ellipsis; white-space: nowrap; }
  .item-row dl { margin: 0; padding: 0 12px 11px 29px; }
  .item-row dl div { display: grid; grid-template-columns: 72px 1fr; gap: 8px; margin-top: 5px; }
  .item-row dt { color: #6e6e73; } .item-row dd { margin: 0; min-width: 0; }
  .report-actions { justify-content: flex-end; margin-top: 16px; }
  .report-actions p { margin: 0 auto 0 0; color: #6e6e73; font-size: 12px; }
  .running-card { min-height: 340px; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; }
  .running-card h3 { margin: 14px 0 6px; }
  .running-card p { color: #6e6e73; }
  .running-card code { margin-bottom: 20px; }
  .running-icon { width: 50px; height: 50px; display: grid; place-items: center; border-radius: 15px; background: rgba(0,122,255,.1); }
  .running-icon span { width: 20px; height: 20px; border: 2px solid rgba(0,122,255,.25); border-top-color: #007aff; border-radius: 50%; animation: spin .8s linear infinite; }
  .progress-track { width: min(460px, 85%); height: 7px; overflow: hidden; margin: 0 0 12px; border-radius: 99px; background: rgba(0,0,0,.08); }
  .progress-track span { display: block; height: 100%; border-radius: inherit; background: #007aff; transition: width .2s ease; }
  .sheet-backdrop { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: rgba(0,0,0,.28); backdrop-filter: blur(5px); }
  .confirm-sheet { width: min(440px, 100%); padding: 24px; border: 1px solid rgba(0,0,0,.12); border-radius: 18px; text-align: center; background: #fff; box-shadow: 0 22px 80px rgba(0,0,0,.22); }
  .confirm-mark { display: grid; place-items: center; width: 46px; height: 46px; margin: 0 auto 14px; border-radius: 50%; color: #007aff; background: rgba(0,122,255,.1); font-size: 25px; }
  .confirm-sheet h2 { margin-bottom: 8px; font-size: 20px; }
  .confirm-sheet > p { color: #6e6e73; font-size: 13px; line-height: 1.45; }
  .confirm-counts { display: flex; justify-content: center; gap: 18px; margin: 18px 0; font-size: 13px; }
  .confirm-actions { justify-content: flex-end; }
  .error-text { color: #d1261c; }
  @media (max-width: 760px) {
    .meeting-main { grid-template-columns: 1fr; gap: 5px; }
    .meeting-meta { flex-wrap: wrap; }
    .mode-field { grid-template-columns: 1fr; }
    .topbar { align-items: flex-start; }
  }
  @media (prefers-color-scheme: dark) {
    :global(body) { --topbar-border: rgba(255,255,255,.11); --topbar-background: rgba(34,34,36,.86); color: #f5f5f7; background: #1c1c1e; }
    .topbar p, .migration-heading p, .field-copy p, .timezone-field p, .meeting-date, .meeting-meta, .report-heading p, .report-actions p, .running-card p, .confirm-sheet > p, .item-path, .item-row dt, .summary-grid span { color: #a1a1a6; }
    .empty-state h2 { color: #f5f5f7; }
    .meeting-row, .setup-card, .report-card, .running-card { border-color: rgba(255,255,255,.11); background: rgba(44,44,46,.84); }
    button.secondary, select, input[type="text"] { border-color: rgba(255,255,255,.16); background: rgba(58,58,60,.8); }
    button.secondary:hover:not(:disabled) { background: #48484a; }
    .selected-path, .summary-grid > div, .mode-badge { background: rgba(255,255,255,.065); }
    .field-block.compact, .timezone-field, .mode-field, .detected-row, .item-row { border-color: rgba(255,255,255,.09); }
    .mode-field > label { border-color: rgba(255,255,255,.13); }
    .mode-field > label.chosen { border-color: #0a84ff; background: rgba(10,132,255,.1); }
    .item-list { border-color: rgba(255,255,255,.1); }
    .progress-track { background: rgba(255,255,255,.1); }
    .confirm-sheet { border-color: rgba(255,255,255,.13); background: #2c2c2e; }
  }
</style>
