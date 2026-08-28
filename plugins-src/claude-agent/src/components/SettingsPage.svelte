<script lang="ts">
  import { onMount } from 'svelte'
  import type { MessageKey } from '../lib/strings'
  import { CONCURRENCY_OPTIONS, loadMaxConcurrency, saveMaxConcurrency } from '../lib/settings'

  const { label }: { label: (key: MessageKey) => string } = $props()
  let maxConcurrency = $state(1)
  let loading = $state(true)
  let saving = $state(false)
  let error = $state('')

  onMount(async () => {
    try {
      maxConcurrency = await loadMaxConcurrency()
    } catch {
      error = label('settings.loadFailed')
    } finally {
      loading = false
    }
  })

  async function change(event: Event): Promise<void> {
    const previous = maxConcurrency
    const next = Number((event.currentTarget as HTMLSelectElement).value)
    maxConcurrency = next
    saving = true
    error = ''
    try {
      maxConcurrency = await saveMaxConcurrency(next)
    } catch {
      maxConcurrency = previous
      error = label('settings.saveFailed')
    } finally {
      saving = false
    }
  }
</script>

<div class="settings-page">
  <h1>{label('settings.title')}</h1>
  <div class="setting-row">
    <div class="copy">
      <label for="max-concurrency">{label('settings.maxConcurrency')}</label>
      <p>{label('settings.maxConcurrencyHint')}</p>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <select id="max-concurrency" value={maxConcurrency} onchange={change} disabled={loading || saving}>
      {#each CONCURRENCY_OPTIONS as option}
        <option value={option}>{option}</option>
      {/each}
    </select>
  </div>
</div>

<style>
  .settings-page { padding: 28px 32px; max-width: 720px; }
  h1 { margin: 0 0 24px; font-size: 22px; }
  .setting-row {
    display: flex;
    align-items: flex-start;
    gap: 24px;
    padding: 16px 0;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .copy { flex: 1; min-width: 220px; }
  label { display: block; font-size: 13px; font-weight: 600; }
  p { margin: 5px 0 0; font-size: 12px; line-height: 1.45; opacity: 0.62; }
  .error { color: #d9534f; opacity: 1; }
  select {
    width: 88px;
    padding: 5px 8px;
    border: 1px solid color-mix(in srgb, currentColor 28%, transparent);
    border-radius: 6px;
    background: Canvas;
    color: CanvasText;
    font: inherit;
  }
  select:disabled { opacity: 0.55; }
  @media (max-width: 700px) {
    .settings-page { padding: 22px 24px; }
    .setting-row { flex-wrap: wrap; }
  }
</style>
