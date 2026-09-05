<script lang="ts">
  import type { NextSettings } from '../lib/settings'
  import { t } from '../lib/strings'

  const {
    settings,
    saving = false,
    onCancel,
    onSave,
  }: {
    settings: NextSettings
    saving?: boolean
    onCancel: () => void
    onSave: (settings: NextSettings) => Promise<void>
  } = $props()

  let wipLimit = $state('')
  let defaultPriority = $state<NextSettings['defaultPriority']>('P2')
  let defaultDueDays = $state('0')
  let defaultContext = $state('')
  let initialized = $state(false)
  let localSaving = $state(false)
  let error = $state('')
  const busy = $derived(saving || localSaving)

  $effect.pre(() => {
    if (initialized) return
    wipLimit = String(settings.wipLimit)
    defaultPriority = settings.defaultPriority
    defaultDueDays = String(settings.defaultDueDays)
    defaultContext = settings.defaultContext
    initialized = true
  })

  function validInteger(value: string, minimum: number): number | null {
    const parsed = Number(value)
    return Number.isSafeInteger(parsed) && parsed >= minimum ? parsed : null
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    error = ''
    const normalizedWipLimit = validInteger(wipLimit, 1)
    const normalizedDueDays = validInteger(defaultDueDays, 0)
    if (normalizedWipLimit === null || normalizedDueDays === null) {
      error = t('settings.validation')
      return
    }

    localSaving = true
    try {
      await onSave({
        wipLimit: normalizedWipLimit,
        defaultPriority,
        defaultDueDays: normalizedDueDays,
        defaultContext,
      })
    } catch {
      error = t('settings.saveError')
    } finally {
      localSaving = false
    }
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key !== 'Escape' || busy) return
    event.preventDefault()
    onCancel()
  }
</script>

<svelte:window onkeydown={keydown} />

<section class="settings-page" data-view="settings" aria-labelledby="next-settings-title">
  <div class="settings-head">
    <div>
      <h2 id="next-settings-title">{t('settings.title')}</h2>
      <p>{t('settings.description')}</p>
    </div>
  </div>

  <form onsubmit={submit} novalidate>
    <label class="setting-row">
      <span class="setting-copy">
        <strong>{t('settings.wipLimit')}</strong>
        <small>{t('settings.wipLimit.help')}</small>
      </span>
      <input name="wipLimit" type="number" min="1" step="1" bind:value={wipLimit} disabled={busy} />
    </label>

    <label class="setting-row">
      <span class="setting-copy">
        <strong>{t('settings.defaultPriority')}</strong>
        <small>{t('settings.defaultPriority.help')}</small>
      </span>
      <select
        name="defaultPriority"
        value={defaultPriority}
        disabled={busy}
        onchange={(event) => defaultPriority = event.currentTarget.value as NextSettings['defaultPriority']}
      >
        <option value="P0">{t('priority.P0')}</option>
        <option value="P1">{t('priority.P1')}</option>
        <option value="P2">{t('priority.P2')}</option>
        <option value="P3">{t('priority.P3')}</option>
      </select>
    </label>

    <label class="setting-row">
      <span class="setting-copy">
        <strong>{t('settings.defaultDueDays')}</strong>
        <small>{t('settings.defaultDueDays.help')}</small>
      </span>
      <input name="defaultDueDays" type="number" min="0" step="1" bind:value={defaultDueDays} disabled={busy} />
    </label>

    <label class="setting-row context-row">
      <span class="setting-copy">
        <strong>{t('settings.defaultContext')}</strong>
        <small>{t('settings.defaultContext.help')}</small>
      </span>
      <input name="defaultContext" type="text" bind:value={defaultContext} placeholder="@computer" disabled={busy} />
    </label>

    {#if error}<p class="settings-error" role="alert">{error}</p>{/if}

    <div class="settings-actions">
      <button type="button" onclick={onCancel} disabled={busy}>{t('common.cancel')}</button>
      <button type="submit" class="primary" disabled={busy}>
        {busy ? t('settings.saving') : t('settings.save')}
      </button>
    </div>
  </form>
</section>

<style>
  .settings-page { width: min(760px, calc(100% - 36px)); box-sizing: border-box; margin: 28px auto 48px; border: 1px solid var(--line); border-radius: 16px; background: var(--card); box-shadow: 0 16px 40px color-mix(in srgb, var(--shadow) 9%, transparent); overflow: hidden; }
  .settings-head { padding: 24px 26px 20px; border-bottom: 1px solid var(--line); }
  h2 { margin: 0; font-size: 20px; letter-spacing: -0.01em; }
  p { margin: 6px 0 0; color: var(--muted); font-size: 12.5px; }
  form { padding: 2px 26px 22px; }
  .setting-row { display: flex; align-items: flex-start; justify-content: space-between; gap: 28px; padding: 18px 0; border-bottom: 1px solid var(--line); }
  .setting-copy { display: grid; gap: 5px; min-width: 0; }
  .setting-copy strong { font-size: 13px; }
  .setting-copy small { max-width: 470px; color: var(--muted); font-size: 12px; line-height: 1.45; }
  input, select { width: 150px; box-sizing: border-box; flex: none; border: 1px solid var(--line-strong); border-radius: 8px; background: var(--input); color: var(--fg); padding: 7px 9px; outline: none; font: inherit; }
  input:focus, select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  input:disabled, select:disabled { opacity: 0.55; }
  .context-row input { width: 210px; }
  .settings-error { margin: 14px 0 0; border-radius: 8px; background: color-mix(in srgb, var(--danger) 10%, transparent); color: var(--danger); padding: 9px 11px; }
  .settings-actions { display: flex; justify-content: flex-end; gap: 8px; padding-top: 18px; }
  .settings-actions button { min-width: 84px; border: 1px solid var(--line-strong); border-radius: 9px; background: var(--card); color: var(--fg); padding: 8px 12px; font-weight: 650; cursor: pointer; }
  .settings-actions button:hover:not(:disabled) { background: var(--hover); }
  .settings-actions button.primary { border-color: var(--accent); background: var(--accent); color: #fff; }
  .settings-actions button.primary:hover:not(:disabled) { filter: brightness(1.06); }
  .settings-actions button:disabled { opacity: 0.5; cursor: default; }
  @media (max-width: 660px) {
    .settings-page { width: calc(100% - 28px); margin-top: 18px; }
    .settings-head { padding: 20px; }
    form { padding: 2px 20px 18px; }
    .setting-row { display: grid; gap: 10px; }
    input, select, .context-row input { width: 100%; }
  }
</style>
