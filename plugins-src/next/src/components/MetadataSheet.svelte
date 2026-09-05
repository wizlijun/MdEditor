<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { parseContextDraft, PRIORITIES, type PlanningMetadata, type Priority } from '../lib/metadata'
  import { t } from '../lib/strings'

  let {
    itemTitle,
    metadata,
    saving,
    onCancel,
    onSubmit,
  }: {
    itemTitle: string
    metadata: PlanningMetadata
    saving: boolean
    onCancel(): void
    onSubmit(input: PlanningMetadata): Promise<void>
  } = $props()

  const initial = untrack(() => metadata)
  let priority = $state<Priority>(initial.priority)
  let due = $state(initial.due ?? '')
  let contextDraft = $state(initial.contexts.join(', '))
  let sheetEl: HTMLDivElement | undefined = $state()
  let formEl: HTMLFormElement | undefined = $state()
  let priorityEl: HTMLSelectElement | undefined = $state()

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    await onSubmit({
      priority,
      ...(due ? { due } : {}),
      contexts: parseContextDraft(contextDraft),
    })
  }

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    queueMicrotask(() => priorityEl?.focus())
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
      const focusable = [...sheetEl.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled)')]
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
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="metadata-title" tabindex="-1" bind:this={sheetEl}>
    <header>
      <div>
        <h2 id="metadata-title">{t('metadata.edit.title', { title: itemTitle })}</h2>
        <p>{t('metadata.edit.help')}</p>
      </div>
      <button type="button" class="close" aria-label={t('common.cancel')} disabled={saving} onclick={onCancel}>×</button>
    </header>
    <form data-form="edit-metadata" bind:this={formEl} onsubmit={submit}>
      <label for="edit-metadata-priority">{t('field.priority')}</label>
      <select id="edit-metadata-priority" name="priority" bind:this={priorityEl} bind:value={priority} disabled={saving}>
        {#each PRIORITIES as value}<option value={value}>{t(`priority.${value}` as never)}</option>{/each}
      </select>
      <label for="edit-metadata-due">{t('field.due')}</label>
      <input id="edit-metadata-due" name="due" type="date" bind:value={due} disabled={saving} />
      <label for="edit-metadata-contexts">{t('field.contexts')}</label>
      <input id="edit-metadata-contexts" name="contexts" bind:value={contextDraft} placeholder={t('field.contexts.placeholder')} disabled={saving} />
      <footer>
        <span>⌘↵ / Ctrl↵</span>
        <div>
          <button type="button" class="cancel" disabled={saving} onclick={onCancel}>{t('common.cancel')}</button>
          <button type="submit" class="submit" disabled={saving}>{t('metadata.edit.save')}</button>
        </div>
      </footer>
    </form>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, #000 32%, transparent); backdrop-filter: blur(8px); }
  .sheet { width: min(520px, 100%); max-height: calc(100vh - 48px); overflow: auto; box-sizing: border-box; border: 1px solid var(--line); border-radius: 20px; background: var(--sheet); color: var(--fg); box-shadow: 0 24px 80px color-mix(in srgb, #000 30%, transparent); }
  header { display: flex; justify-content: space-between; gap: 16px; padding: 22px 24px 16px; }
  h2 { margin: 0; font-size: 19px; line-height: 1.3; }
  header p { margin: 5px 0 0; color: var(--muted); font-size: 12px; }
  .close { width: 28px; height: 28px; border: none; border-radius: 50%; background: var(--chip); color: var(--muted-strong); font-size: 20px; line-height: 1; cursor: pointer; }
  form { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 12px 14px; padding: 20px 24px 22px; border-top: 1px solid var(--line); }
  label { font-size: 12px; font-weight: 650; }
  input, select { width: 100%; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 11px; background: var(--input); color: var(--fg); padding: 10px 12px; font: inherit; outline: none; }
  input:focus, select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  footer { grid-column: 1 / -1; display: flex; align-items: center; justify-content: space-between; gap: 12px; padding-top: 8px; }
  footer > span { color: var(--muted); font-size: 12px; }
  footer > div { display: flex; gap: 8px; }
  footer button { border-radius: 9px; padding: 8px 14px; font: inherit; font-weight: 650; cursor: pointer; }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .cancel { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .submit { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  @media (max-width: 520px) {
    form { grid-template-columns: 1fr; }
    footer { grid-column: 1; }
  }
</style>
