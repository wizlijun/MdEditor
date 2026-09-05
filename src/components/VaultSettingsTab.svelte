<script lang="ts">
  import { vaultStore, syncNow, configureVault, disconnectVault, refreshStatus, fetchGitHubLogin } from '../lib/vault.svelte'
  import { pushToast } from '../lib/toast.svelte'
  import { ask } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { t } from '../lib/i18n/store.svelte'
  import { onDestroy } from 'svelte'

  let { onBusyChange }: { onBusyChange?: (busy: boolean) => void } = $props()

  let remoteUrl = $state('')
  let branch = $state('main')
  let pat = $state('')
  let authorName = $state('note.md on iOS')
  let authorEmail = $state('')
  let busy = $state(false)
  let saveError = $state<string | null>(null)
  let showPatInput = $state(false)
  const canSave = $derived(!busy && !!remoteUrl && (vaultStore.configured || !!pat))
  $effect(() => { onBusyChange?.(busy) })
  onDestroy(() => onBusyChange?.(false))

  $effect(() => { refreshStatus() })

  // When the user finishes typing a PAT, try to fetch their GitHub login
  // and auto-fill the noreply email (spec §2.5). Debounced 800ms; only
  // overwrite empty email field, never clobber user-entered value.
  let emailFetchTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    if (!pat || pat.length < 20) return
    if (emailFetchTimer) clearTimeout(emailFetchTimer)
    emailFetchTimer = setTimeout(async () => {
      if (authorEmail.trim() !== '') return
      const login = await fetchGitHubLogin(pat)
      if (login && authorEmail.trim() === '') {
        authorEmail = `${login}@users.noreply.github.com`
      }
    }, 800)
    return () => { if (emailFetchTimer) clearTimeout(emailFetchTimer) }
  })

  async function onSave() {
    if (!canSave) return
    saveError = null
    busy = true
    try {
      await configureVault({ remoteUrl, branch, pat, authorName, authorEmail })
      // 配好 vault 后进程内即时生效:刷新前端 vault 状态,分享/徽标等无需重启 app。
      const { refreshSotvault } = await import('../lib/sotvault.svelte')
      await refreshSotvault()
      showPatInput = false
      pat = ''
      pushToast({ level: 'success', message: t('vault.connected') })
    } catch (e) {
      const raw = typeof e === 'string' ? e : String(e)
      saveError = raw
      // Map known error patterns to friendlier toast text.
      let friendly = t('vault.err.generic', { error: raw })
      if (raw.includes('keychain') || raw.includes('plugin:keychain')) {
        friendly = t('vault.err.keychain')
      } else if (raw.includes('auth') || raw.includes('鉴权') || raw.includes('401')) {
        friendly = t('vault.err.authConnect')
      } else if (raw.includes('404') || raw.includes('not found')) {
        friendly = t('vault.err.notFoundConnect')
      } else if (raw.includes('network') || raw.includes('网络')) {
        friendly = t('vault.err.networkConnect')
      }
      pushToast({ level: 'error', message: friendly, detail: raw })
    } finally {
      busy = false
    }
  }

  async function onDisconnect() {
    const ok = await ask(t('vault.disconnectConfirm'), {
      title: t('vault.disconnectTitle'), kind: 'warning',
    })
    if (!ok) return
    busy = true
    try {
      await disconnectVault()
      pushToast({ level: 'success', message: t('vault.disconnected') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vault.disconnectFailed', { error: String(e) }), detail: String(e) })
    } finally {
      busy = false
    }
  }

  function formatLastSync(ms: number | null): string {
    if (!ms) return t('time.never')
    const diff = Date.now() - ms
    if (diff < 60_000) return t('time.justNow')
    if (diff < 3_600_000) return t('time.minutesAgo', { n: Math.round(diff / 60_000) })
    if (diff < 86_400_000) return t('time.hoursAgo', { n: Math.round(diff / 3_600_000) })
    return new Date(ms).toLocaleString()
  }

  async function openTokenPage() {
    try { await openUrl('https://github.com/settings/personal-access-tokens/new') } catch {}
  }
</script>

<section class="vault-settings ui-surface" aria-label={t('settings.tab.vault')}>
  <div class="status-block">
    <div class="status-row">
      <span class="label">{t('vault.statusLabel')}</span>
      <span class="state state-{vaultStore.state}" role="status" aria-live="polite">
        {#if vaultStore.state === 'syncing'}{t('vault.syncing')}
        {:else if vaultStore.state === 'cloning'}{t('vault.cloning')}
        {:else if vaultStore.state === 'idle'}{t('vault.lastSync', { time: formatLastSync(vaultStore.lastSync) })}
        {:else if vaultStore.state === 'error'}{vaultStore.errorMsg ?? t('vault.unknownError')}
        {:else if vaultStore.state === 'conflict'}{t('vault.hasConflicts')}
        {:else}{t('vault.notConfigured')}
        {/if}
      </span>
    </div>
    {#if vaultStore.configured}
      <div class="actions">
        <button onclick={() => syncNow()} disabled={busy || vaultStore.state === 'syncing'}>
          {vaultStore.state === 'syncing' ? t('vault.syncing') : t('vault.syncNow')}
        </button>
        <button class="danger" onclick={onDisconnect} disabled={busy}>{t('vault.disconnect')}</button>
      </div>
    {/if}
  </div>

  <hr />

  <form class="form" aria-busy={busy} onsubmit={(event) => { event.preventDefault(); void onSave() }}>
    <fieldset disabled={busy}>
    <label>
      <span>{t('vault.remoteUrl')}</span>
      <input type="text" bind:value={remoteUrl} placeholder="https://github.com/user/repo.git" spellcheck="false" autocapitalize="off" />
    </label>
    <label>
      <span>{t('vault.branch')}</span>
      <input type="text" bind:value={branch} placeholder="main" spellcheck="false" autocapitalize="off" />
    </label>
    <div class="pat-row field">
      <span id="vault-pat-label">{t('vault.pat')}</span>
      {#if !showPatInput && vaultStore.configured}
        <div>
          <span class="badge ok">{t('vault.patConfigured')}</span>
          <button type="button" class="link" onclick={() => (showPatInput = true)}>{t('vault.patUpdate')}</button>
        </div>
      {:else}
        <input type="password" aria-labelledby="vault-pat-label" bind:value={pat} placeholder="github_pat_..." autocomplete="new-password" spellcheck="false" />
      {/if}
      <button type="button" class="link" onclick={openTokenPage}>{t('vault.howToToken')}</button>
    </div>
    <label>
      <span>{t('vault.authorName')}</span>
      <input type="text" bind:value={authorName} />
    </label>
    <label>
      <span>{t('vault.authorEmail')}</span>
      <input type="text" inputmode="email" bind:value={authorEmail} placeholder="user@users.noreply.github.com" autocapitalize="off" />
    </label>
    <button type="submit" class="primary" disabled={!canSave}>
      {busy ? t('vault.saving') : t('vault.saveConfig')}
    </button>
    </fieldset>
    {#if saveError}
      <p class="error" role="alert">{saveError}</p>
    {/if}
  </form>

  <hr />

  <p class="note">{t('vault.filesWarning')}</p>
</section>

<style>
  .vault-settings { padding: 0; min-width: 0; }
  .status-block { padding: 14px; background: var(--ui-bg); border: 1px solid var(--ui-separator); border-radius: 8px; }
  .status-row { display: flex; flex-wrap: wrap; gap: 8px; }
  .label { font-weight: 500; color: var(--ui-secondary); }
  .state { min-width: 0; overflow-wrap: anywhere; }
  .state-error { color: var(--ui-danger); }
  .state-conflict { color: var(--ui-warning); }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  button { padding: 7px 12px; min-height: 32px; font: inherit; border: 1px solid var(--ui-control-border); border-radius: 6px; color: CanvasText; background: var(--ui-surface, Canvas); cursor: pointer; }
  button:not(:disabled):hover { background: var(--ui-hover); }
  button:disabled { opacity: 0.5; cursor: default; }
  .danger { color: var(--ui-danger); }
  hr { border: 0; border-top: 1px solid var(--ui-separator); margin: 20px 0; }
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  .form label, .field { display: flex; flex-direction: column; align-items: stretch; gap: 6px; margin-bottom: 16px; }
  .form label > span, .field > span { font-size: 13px; font-weight: 500; }
  .form input { width: 100%; min-width: 0; min-height: 34px; padding: 7px 10px; border: 1px solid var(--ui-control-border); border-radius: 6px; font: inherit; background: var(--ui-surface, Canvas); color: CanvasText; }
  .badge.ok { color: var(--ui-success); }
  .link { align-self: flex-start; background: transparent; border: 0; padding: 4px 0; color: var(--ui-accent-text); text-decoration: underline; text-underline-offset: 3px; cursor: pointer; font-size: 12px; }
  .primary { padding: 8px 16px; background: var(--ui-accent); color: var(--ui-accent-foreground, white); border-color: var(--ui-accent); }
  button.primary:not(:disabled):hover { background: color-mix(in srgb, var(--ui-accent) 88%, black); }
  .primary:disabled { opacity: 0.5; cursor: not-allowed; }
  .pat-row > div { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .error { color: var(--ui-danger); margin-top: 12px; font-size: 13px; overflow-wrap: anywhere; }
  .note { font-size: 12px; line-height: 1.5; color: var(--ui-secondary); overflow-wrap: anywhere; }
</style>
