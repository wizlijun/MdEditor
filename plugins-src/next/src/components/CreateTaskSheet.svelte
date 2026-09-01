<script lang="ts">
  import { onMount } from 'svelte'
  import { t } from '../lib/strings'

  export interface TaskDraft {
    title: string
    body?: string
    done_when?: string
  }

  let {
    taskDir,
    saving,
    onCancel,
    onSubmit,
  }: {
    taskDir: string
    saving: boolean
    onCancel(): void
    onSubmit(input: TaskDraft, markCurrent: boolean): Promise<void>
  } = $props()

  let title = $state('')
  let body = $state('')
  let doneWhen = $state('')
  let invalid = $state<'title' | 'done_when' | null>(null)
  let sheetEl: HTMLDivElement | undefined = $state()
  let formEl: HTMLFormElement | undefined = $state()
  let titleEl: HTMLInputElement | undefined = $state()
  let doneWhenEl: HTMLInputElement | undefined = $state()

  function draft(): TaskDraft {
    const details = body.trim()
    const close = doneWhen.trim()
    return {
      title: title.trim(),
      ...(details ? { body: details } : {}),
      ...(close ? { done_when: close } : {}),
    }
  }

  async function submitInbox(event: SubmitEvent) {
    event.preventDefault()
    const input = draft()
    if (!input.title) {
      invalid = 'title'
      titleEl?.focus()
      return
    }
    invalid = null
    await onSubmit(input, false)
  }

  async function submitCurrent() {
    const input = draft()
    if (!input.title) {
      invalid = 'title'
      titleEl?.focus()
      return
    }
    if (!input.done_when) {
      invalid = 'done_when'
      doneWhenEl?.focus()
      return
    }
    invalid = null
    await onSubmit(input, true)
  }

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    queueMicrotask(() => titleEl?.focus())
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) {
        event.preventDefault()
        onCancel()
        return
      }
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !saving) {
        event.preventDefault()
        formEl?.requestSubmit()
        return
      }
      if (event.key !== 'Tab' || !sheetEl) return
      const focusable = [...sheetEl.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled)')]
      if (!focusable.length) return
      const first = focusable[0]
      const last = focusable.at(-1)!
      if (!focusable.includes(document.activeElement as HTMLElement)) {
        event.preventDefault()
        ;(event.shiftKey ? last : first).focus()
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      previousFocus?.focus()
    }
  })
</script>

<div class="scrim" role="presentation" onclick={(event) => event.target === event.currentTarget && !saving && onCancel()}>
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="create-task-title" tabindex="-1" bind:this={sheetEl}>
    <header>
      <div>
        <h2 id="create-task-title">{t('task.create.title')}</h2>
        <p>{t('task.create.destination', { path: taskDir })}</p>
      </div>
      <button type="button" class="close" aria-label={t('common.cancel')} disabled={saving} onclick={onCancel}>×</button>
    </header>
    <form data-form="create-task" bind:this={formEl} onsubmit={submitInbox}>
      <label for="new-task-title">{t('task.create.field')}</label>
      <input
        id="new-task-title"
        name="title"
        bind:this={titleEl}
        bind:value={title}
        aria-invalid={invalid === 'title'}
        placeholder={t('task.create.placeholder')}
      />

      <label for="new-task-body">{t('task.create.body')} <span>{t('common.optional')}</span></label>
      <textarea
        id="new-task-body"
        name="body"
        bind:value={body}
        placeholder={t('task.create.bodyPlaceholder')}
        rows="4"
      ></textarea>

      <label for="new-task-done">{t('task.create.doneWhen')} <span>{t('common.optional')}</span></label>
      <input
        id="new-task-done"
        name="done_when"
        bind:this={doneWhenEl}
        bind:value={doneWhen}
        aria-invalid={invalid === 'done_when'}
        placeholder={t('task.create.doneWhenPlaceholder')}
      />

      {#if invalid === 'title'}
        <p class="error" role="alert">{t('error.taskRequired')}</p>
      {:else if invalid === 'done_when'}
        <p class="error" role="alert">{t('error.doneWhenRequired')}</p>
      {/if}

      <footer>
        <span>{t('task.create.saveShortcut')}</span>
        <div>
          <button type="button" class="cancel" disabled={saving} onclick={onCancel}>{t('common.cancel')}</button>
          <button type="submit" class="inbox" disabled={saving}>{t('task.create.saveInbox')}</button>
          <button type="button" data-action="create-current" class="submit" disabled={saving} onclick={submitCurrent}>{t('task.create.saveCurrent')}</button>
        </div>
      </footer>
    </form>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, #000 32%, transparent); backdrop-filter: blur(8px); }
  .sheet { width: min(620px, 100%); box-sizing: border-box; border: 1px solid var(--line); border-radius: 20px; background: var(--sheet); color: var(--fg); box-shadow: 0 24px 80px color-mix(in srgb, #000 30%, transparent); }
  header { display: flex; justify-content: space-between; gap: 16px; padding: 22px 24px 15px; }
  h2 { margin: 0; font-size: 19px; line-height: 1.3; }
  header p { margin: 5px 0 0; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
  .close { width: 28px; height: 28px; border: none; border-radius: 50%; background: var(--chip); color: var(--muted-strong); font-size: 20px; line-height: 1; cursor: pointer; }
  form { display: grid; gap: 8px; padding: 18px 24px 22px; border-top: 1px solid var(--line); }
  label { margin-top: 5px; font-size: 12px; font-weight: 650; }
  label span { color: var(--muted); font-weight: 450; }
  input, textarea { width: 100%; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 11px; background: var(--input); color: var(--fg); padding: 10px 12px; font: inherit; line-height: 1.45; outline: none; }
  textarea { resize: vertical; min-height: 88px; }
  input:focus, textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  [aria-invalid="true"] { border-color: var(--danger); }
  .error { margin: 2px 0 0; color: var(--danger); font-size: 12px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 12px; }
  footer > span { color: var(--muted); font-size: 11px; }
  footer > div { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
  footer button { border-radius: 9px; padding: 8px 12px; font: inherit; font-weight: 650; cursor: pointer; }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .cancel { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .inbox { border: 1px solid var(--line-strong); background: var(--card); color: var(--fg); }
  .submit { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  @media (max-width: 680px) {
    footer { align-items: stretch; flex-direction: column; }
    footer > div { justify-content: stretch; }
    footer button { flex: 1; }
  }
</style>
