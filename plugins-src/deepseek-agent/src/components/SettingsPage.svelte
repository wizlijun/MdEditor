<script lang="ts">
  import { onMount, tick } from 'svelte'
  import type { MessageKey } from '../lib/strings'
  import {
    CONCURRENCY_OPTIONS,
    USAGE_DISPLAY_OPTIONS,
    loadMaxConcurrency,
    loadUsageDisplay,
    saveMaxConcurrency,
    saveUsageDisplay,
    type UsageDisplay,
  } from '../lib/settings'

  const { label }: { label: (key: MessageKey) => string } = $props()
  let maxConcurrency = $state(1)
  let usageDisplay: UsageDisplay = $state('tip')
  let loading = $state(true)
  let saving = $state(false)
  let error = $state('')
  let loadFailed = $state(false)
  let saved = $state(false)

  async function loadSettings() {
    loading = true
    loadFailed = false
    error = ''
    saved = false
    try {
      ;[maxConcurrency, usageDisplay] = await Promise.all([
        loadMaxConcurrency(),
        loadUsageDisplay(),
      ])
    } catch {
      loadFailed = true
      error = label('settings.loadFailed')
    } finally {
      loading = false
    }
  }
  onMount(() => { void loadSettings() })

  async function change(event: Event): Promise<void> {
    const control = event.currentTarget as HTMLSelectElement
    const restoreFocus = document.activeElement === control
    const previous = maxConcurrency
    const next = Number((event.currentTarget as HTMLSelectElement).value)
    maxConcurrency = next
    saving = true
    saved = false
    error = ''
    try {
      maxConcurrency = await saveMaxConcurrency(next)
      saved = true
    } catch {
      maxConcurrency = previous
      error = label('settings.saveFailed')
    } finally {
      saving = false
      await tick()
      if (restoreFocus && control.isConnected && document.activeElement === document.body) control.focus()
    }
  }

  async function changeUsageDisplay(event: Event): Promise<void> {
    const control = event.currentTarget as HTMLSelectElement
    const restoreFocus = document.activeElement === control
    const previous = usageDisplay
    usageDisplay = (event.currentTarget as HTMLSelectElement).value as UsageDisplay
    saving = true
    saved = false
    error = ''
    try {
      usageDisplay = await saveUsageDisplay(usageDisplay)
      saved = true
    } catch {
      usageDisplay = previous
      error = label('settings.saveFailed')
    } finally {
      saving = false
      await tick()
      if (restoreFocus && control.isConnected && document.activeElement === document.body) control.focus()
    }
  }
</script>

<div class="settings-page" aria-busy={loading || saving}>
  <h1>{label('settings.title')}</h1>
  <p class="intro">{label('settings.autosave')}</p>
  <div class="settings-group">
    <div class="setting-row">
      <div class="copy">
        <label for="max-concurrency">{label('settings.maxConcurrency')}</label>
        <p id="max-concurrency-hint">{label('settings.maxConcurrencyHint')}</p>
      </div>
      <select id="max-concurrency" aria-describedby="max-concurrency-hint" value={maxConcurrency} onchange={change} disabled={loading || saving || loadFailed}>
        {#each CONCURRENCY_OPTIONS as option}<option value={option}>{option}</option>{/each}
      </select>
    </div>
    <div class="setting-row">
      <div class="copy">
        <label for="usage-display">{label('settings.usageDisplay')}</label>
        <p id="usage-display-hint">{label('settings.usageDisplayHint')}</p>
      </div>
      <select id="usage-display" aria-describedby="usage-display-hint" value={usageDisplay} onchange={changeUsageDisplay} disabled={loading || saving || loadFailed}>
        {#each USAGE_DISPLAY_OPTIONS as option}
          <option value={option}>{label(option === 'tip' ? 'settings.usageDisplayTip' : 'settings.usageDisplayResult')}</option>
        {/each}
      </select>
    </div>
  </div>
  <div class="feedback">
    {#if error}
      <p class="error" role="alert">{error}</p>
      {#if loadFailed}<button type="button" onclick={loadSettings}>{label('settings.retry')}</button>{/if}
    {:else}
      <p role="status">{loading ? label('settings.loading') : saving ? label('settings.saving') : saved ? label('settings.saved') : ''}</p>
    {/if}
  </div>
</div>

<style>
  .settings-page { height: 100%; min-width: 0; overflow: auto; box-sizing: border-box; padding: 24px 24px 36px; }
  h1 { max-width: 720px; margin: 0 auto 6px; font-size: 22px; letter-spacing: -0.025em; }
  .intro { max-width: 720px; margin: 0 auto 20px; }
  .settings-group { max-width: 720px; margin: 0 auto; border: 1px solid var(--ui-separator); border-radius: 12px; background: var(--ui-surface); }
  .setting-row { display: flex; align-items: flex-start; flex-wrap: wrap; gap: 12px 24px; padding: 16px; }
  .setting-row + .setting-row { border-top: 1px solid var(--ui-separator); }
  .copy { flex: 1 1 220px; min-width: 0; overflow-wrap: anywhere; }
  label { display: block; font-size: 13px; font-weight: 600; }
  p { margin: 5px 0 0; font-size: 12px; line-height: 1.5; color: var(--ui-secondary); }
  select, button { min-height: 32px; padding: 6px 9px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: CanvasText; font: inherit; }
  select { min-width: 120px; max-width: 100%; }
  select:disabled { opacity: 0.55; }
  button { margin-top: 8px; cursor: pointer; }
  button:hover { background: var(--ui-hover); }
  .feedback { max-width: 720px; min-height: 28px; margin: 8px auto 0; }
  .error { color: var(--ui-danger); }
  @media (max-width: 700px) { .settings-page { padding: 18px 16px 28px; } }
</style>
