<script lang="ts">
  import { onMount } from 'svelte'
  import { t } from '../lib/strings'

  let {
    ideaDir,
    saving,
    onCancel,
    onSubmit,
  }: {
    ideaDir: string
    saving: boolean
    onCancel(): void
    onSubmit(body: string): Promise<void>
  } = $props()

  let body = $state('')
  let invalid = $state(false)
  let sheetEl: HTMLDivElement | undefined = $state()
  let formEl: HTMLFormElement | undefined = $state()
  let textareaEl: HTMLTextAreaElement | undefined = $state()

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (!body.trim()) {
      invalid = true
      textareaEl?.focus()
      return
    }
    invalid = false
    await onSubmit(body)
  }

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    queueMicrotask(() => textareaEl?.focus())
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
      const focusable = [...sheetEl.querySelectorAll<HTMLElement>('button:not(:disabled), textarea:not(:disabled)')]
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
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="create-title" tabindex="-1" bind:this={sheetEl}>
    <header>
      <div>
        <h2 id="create-title">{t('create.title')}</h2>
        <p>{t('create.destination', { path: ideaDir })}</p>
      </div>
      <button class="close" aria-label={t('common.cancel')} disabled={saving} onclick={onCancel}>×</button>
    </header>
    <form data-form="create-idea" bind:this={formEl} onsubmit={submit}>
      <label for="new-idea">{t('create.field')}</label>
      <textarea
        id="new-idea"
        name="idea"
        bind:this={textareaEl}
        bind:value={body}
        aria-invalid={invalid}
        placeholder={t('create.placeholder')}
        rows="8"
      ></textarea>
      {#if invalid}<p class="error" role="alert">{t('error.ideaRequired')}</p>{/if}
      <footer>
        <span>{t('create.saveShortcut')}</span>
        <div>
          <button type="button" class="cancel" disabled={saving} onclick={onCancel}>{t('common.cancel')}</button>
          <button type="submit" class="submit" disabled={saving}>{t('create.save')}</button>
        </div>
      </footer>
    </form>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, #000 32%, transparent); backdrop-filter: blur(8px); }
  .sheet { width: min(560px, 100%); box-sizing: border-box; border: 1px solid var(--line); border-radius: 20px; background: var(--sheet); color: var(--fg); box-shadow: 0 24px 80px color-mix(in srgb, #000 30%, transparent); }
  header { display: flex; justify-content: space-between; gap: 16px; padding: 22px 24px 15px; }
  h2 { margin: 0; font-size: 19px; line-height: 1.3; }
  header p { margin: 5px 0 0; color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
  .close { width: 28px; height: 28px; border: none; border-radius: 50%; background: var(--chip); color: var(--muted-strong); font-size: 20px; line-height: 1; cursor: pointer; }
  form { display: grid; gap: 8px; padding: 18px 24px 22px; border-top: 1px solid var(--line); }
  label { font-size: 12px; font-weight: 650; }
  textarea { width: 100%; resize: vertical; min-height: 132px; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 11px; background: var(--input); color: var(--fg); padding: 11px 12px; font: inherit; line-height: 1.55; outline: none; }
  textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  textarea[aria-invalid="true"] { border-color: var(--danger); }
  .error { margin: 0; color: var(--danger); font-size: 12px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 8px; }
  footer > span { color: var(--muted); font-size: 11px; }
  footer > div { display: flex; gap: 8px; }
  footer button { border-radius: 9px; padding: 8px 14px; font: inherit; font-weight: 650; cursor: pointer; }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .cancel { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .submit { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
</style>
