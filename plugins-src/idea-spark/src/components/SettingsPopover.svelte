<!-- SettingsPopover.svelte — the gear's only setting: which vault directory
     ideas are saved into. Validation is live and non-destructive (the save
     button greys out for an empty / absolute / `..`-bearing path via
     `normalizeIdeaDir`), so the store is only ever handed a value that already
     passed the same check `setIdeaDir` applies.

     `onbeforecommit` is the flush barrier, and it is not optional: a real
     directory change DETACHES the open document (`changeIdeaDir` clears
     `current`/`currentFrontmatter`), so a buffer that is still dirty when the
     save button is pressed would be written by the next autosave tick as a
     BRAND NEW file in the NEW directory, with freshly stamped frontmatter,
     while the original keeps the old text — one idea silently forked in two.
     This is the only place where a settings action can produce a file.

     It returns a BOOLEAN, and that is the whole point: flushing is not the
     same as having flushed. `saveNow()` never rejects (`autosave.ts` swallows
     the write's failure by design), so merely awaiting it would let a failed
     write through and change the directory anyway — the same fork, narrowed
     to the "the disk said no" branch. The callback has to assert the
     postcondition and answer yes/no; `false` aborts, leaving the popover open
     on the field the user was editing. -->
<script lang="ts">
  import { commitIdeaDir, normalizeIdeaDir, openPrompt, state as store } from '../lib/store.svelte'
  import { TASK_ID } from '../lib/agent-client'
  import { t } from '../lib/strings'

  const {
    onclose,
    onbeforecommit,
  }: {
    onclose: () => void
    onbeforecommit?: () => Promise<boolean>
  } = $props()

  /** 这个插件唯一的可编辑提示词:论证任务的 CLAUDE.md。 */
  const prompts = $derived([{ taskId: TASK_ID, label: t('promptMain') }])

  // Editing a prompt is not a setting change: nothing here is committed, and
  // the idea buffer is not detached — so no flush barrier, unlike `commit`.
  // The popover closes because the main window is about to take focus and a
  // popover left open behind it would be waiting for a click nobody will make.
  async function editPrompt(taskId: string): Promise<void> {
    onclose()
    await openPrompt(taskId)
  }

  let value = $state(store.ideaDir)
  const valid = $derived(normalizeIdeaDir(value) !== null)

  async function commit(): Promise<void> {
    if (!valid) return
    // No callback ⇒ nothing to flush ⇒ proceed. Defaulted to an always-yes
    // rather than optional-called (`onbeforecommit?.()`), because that yields
    // `undefined`, which is falsy — an absent barrier would read as a refusal
    // and the setting could never be saved.
    const ok = await commitIdeaDir(value, onbeforecommit ?? (async () => true))
    // Left open on purpose when the commit was refused — by the flush barrier
    // (the user still has unsaved text, and a toast has said so) or by the
    // store (see `saveIdeaDir`: it returns false "so the popover can keep the
    // field open"). Closing would hide a change that did not happen.
    if (ok) onclose()
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
  <!-- 委托提示词:一行一个 task 模板的 CLAUDE.md,点开就是标准 md 编辑器。 -->
  <div class="section">
    <span class="label" id="prompts-label">{t('prompts')}</span>
    <ul class="prompts" aria-labelledby="prompts-label">
      {#each prompts as p (p.taskId)}
        <li>
          <button type="button" class="row" onclick={() => editPrompt(p.taskId)}>
            <span class="name">{p.label}</span>
            <span class="path" aria-hidden="true">{p.taskId}/CLAUDE.md</span>
          </button>
        </li>
      {/each}
    </ul>
    <p class="hint">{t('promptsHint')}</p>
  </div>

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
    font-size: 0.7rem;
    opacity: 0.55;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    margin: 0.45rem 0 0;
    font-size: 0.7rem;
    line-height: 1.4;
    opacity: 0.6;
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
