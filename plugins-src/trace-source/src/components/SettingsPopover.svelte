<!-- SettingsPopover.svelte — the gear's two concerns: which vault directory
     trace reports land in, and the delegation prompt (a file you own).

     Same shape as idea-spark's settings popover. App flushes and detaches any
     Inbox document before changing directories; a failed flush leaves this
     popover open. Validation is live and non-destructive (the save button
     greys out for an empty / absolute / `..`-bearing path).

     Reports already written stay where they are; the inbox lists the
     directory the setting points at NOW, which is the honest reading of
     "report folder". -->
<script lang="ts">
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import { normalizeTraceDir } from '../lib/state-io'
  import { t } from '../lib/strings'

  const {
    traceDir,
    onclose,
    oncommit,
    oneditprompt,
  }: {
    traceDir: string
    onclose: () => void
    /** Applies a validated directory; the App owns persistence + relist. */
    oncommit: (dir: string) => void | boolean | Promise<void | boolean>
    /** Opens the trace task's CLAUDE.md in the main editor (App seeds first). */
    oneditprompt: () => void | Promise<void>
  } = $props()

  // The popover edits a snapshot on purpose: the field starts from the value
  // at open, and only `oncommit` writes back.
  // svelte-ignore state_referenced_locally
  let value = $state(traceDir)
  let saving = $state(false)
  let error = $state('')
  const valid = $derived(normalizeTraceDir(value) !== null)

  async function commit(): Promise<void> {
    const normalized = normalizeTraceDir(value)
    if (normalized === null || saving) return
    saving = true
    error = ''
    try {
      if ((await oncommit(normalized)) !== false) onclose()
    } catch (cause) {
      error = String(cause)
    } finally {
      saving = false
    }
  }

  function close(): void { if (!saving) onclose() }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); void commit() }
  }

  // The popover closes because the main window is about to take focus — a
  // popover left open behind it would be waiting for a click nobody makes.
  async function editPrompt(): Promise<void> {
    onclose()
    await oneditprompt()
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={close}></div>
<div class="popover" role="dialog" aria-modal="true" aria-busy={saving} aria-label={t('settings')} use:modalFocus={{ onClose: close, canClose: () => !saving }}>
  <label for="trace-dir">{t('traceDir')}</label>
  <input
    id="trace-dir"
    type="text"
    bind:value
    disabled={saving}
    aria-invalid={!valid}
    class:invalid={!valid}
    spellcheck="false"
    autocomplete="off"
    {onkeydown}
  />
  <!-- 委托提示词:溯源任务的 CLAUDE.md,点开就是标准 md 编辑器。 -->
  <div class="section">
    <span class="label" id="prompts-label">{t('prompts')}</span>
    <ul class="prompts" aria-labelledby="prompts-label">
      <li>
        <button type="button" class="row" disabled={saving} onclick={editPrompt}>
          <span class="name">{t('promptMain')}</span>
          <span class="path" aria-hidden="true">trace-source/CLAUDE.md</span>
        </button>
      </li>
    </ul>
    <p class="hint">{t('promptsHint')}</p>
  </div>

  <div class="actions">
    <button type="button" class="ghost" disabled={saving} onclick={close}>{t('close')}</button>
    <button type="button" class="primary" disabled={!valid || saving} onclick={commit}>
      {t('save')}
    </button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
  }
  .popover {
    position: absolute;
    /* Anchored to the bottom action bar (App.svelte `.actionbar`, which is
       `position: relative`), so it opens upward. */
    bottom: 2.4rem;
    right: 0.75rem;
    z-index: 11;
    width: min(360px, calc(100vw - 24px));
    max-height: calc(100vh - 64px);
    overflow-y: auto;
    box-sizing: border-box;
    padding: 0.75rem;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 8px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  label,
  .label {
    display: block;
    font-size: 0.75rem;
    opacity: 0.7;
    margin-bottom: 0.3rem;
  }
  .section {
    margin-top: 0.9rem;
    padding-top: 0.75rem;
    border-top: 1px solid var(--line, #e5e7eb);
  }
  .prompts {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    width: 100%;
    padding: 0.3rem 0.4rem;
    border: 1px solid transparent;
    border-radius: 6px;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font: inherit;
  }
  .row:hover,
  .row:focus-visible {
    background: color-mix(in srgb, CanvasText 8%, transparent);
  }
  .name {
    font-size: 0.85rem;
  }
  /* The file behind the row — the point of "the prompt is a file you own". */
  .path {
    margin-left: auto;
    font-size: 12px;
    color: var(--ui-secondary);
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .hint {
    margin: 0.45rem 0 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--ui-secondary);
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
  .error { color: var(--ui-danger); font-size: 12px; overflow-wrap: anywhere; }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
    margin-top: 0.7rem;
  }
  .actions button {
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
  .actions button:disabled { opacity: 0.5; cursor: default; }
</style>
