<!-- SettingsPopover.svelte — the gear's only setting: which vault directory
     ideas are saved into. Validation is live and non-destructive (the save
     button greys out for an empty / absolute / `..`-bearing path via
     `normalizeIdeaDir`), so the store is only ever handed a value that already
     passed the same check `setIdeaDir` applies. -->
<script lang="ts">
  import { normalizeIdeaDir, saveIdeaDir, state as store } from '../lib/store.svelte'
  import { t } from '../lib/strings'

  const { onclose }: { onclose: () => void } = $props()

  let value = $state(store.ideaDir)
  const valid = $derived(normalizeIdeaDir(value) !== null)

  async function commit(): Promise<void> {
    if (!valid) return
    await saveIdeaDir(value)
    onclose()
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') onclose()
    else if (e.key === 'Enter') void commit()
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}></div>
<div class="popover" role="dialog" aria-label={t('settings')}>
  <label for="idea-dir">{t('ideaDir')}</label>
  <input
    id="idea-dir"
    type="text"
    bind:value
    aria-invalid={!valid}
    class:invalid={!valid}
    spellcheck="false"
    autocomplete="off"
    {onkeydown}
  />
  <div class="actions">
    <button type="button" class="ghost" onclick={onclose}>{t('close')}</button>
    <button type="button" class="primary" disabled={!valid || store.busy} onclick={commit}>
      {t('save')}
    </button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
  }
  .popover {
    position: absolute;
    /* Anchored to the *bottom* action bar (App.svelte `.actionbar`, which is
       `position: relative`), so it opens upward — measuring from the top would
       push it off the bottom edge of the window. */
    bottom: 2.4rem;
    right: 0.75rem;
    z-index: 11;
    width: 300px;
    box-sizing: border-box;
    padding: 0.75rem;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  label {
    display: block;
    font-size: 0.75rem;
    opacity: 0.7;
    margin-bottom: 0.3rem;
  }
  input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.35rem 0.45rem;
    border: 1px solid var(--line, #d1d5db);
    border-radius: 6px;
    background: Field;
    color: FieldText;
    /* input does not inherit font-size/family either (MEMORY note). */
    font: inherit;
    font-size: 0.85rem;
  }
  input.invalid { border-color: #dc2626; }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
    margin-top: 0.7rem;
  }
  button {
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--line, #d1d5db);
    background: none;
    color: inherit;
  }
  .primary {
    border: 1px solid transparent;
    background: var(--accent, #2563eb);
    color: #fff;
  }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
