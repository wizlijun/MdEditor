<script lang="ts">
  import { onMount } from 'svelte'
  import type { WorkspaceItem } from '../lib/repository'
  import type { IdeaSource } from '../lib/source'
  import { t } from '../lib/strings'

  let {
    item,
    saving,
    onCancel,
    onSubmit,
  }: {
    item: WorkspaceItem
    saving: boolean
    onCancel(): void
    onSubmit(source: IdeaSource): Promise<void>
  } = $props()

  let query = $state('')
  let selected = $state<string | null>(null)
  let sheetEl: HTMLDivElement | undefined = $state()
  const candidates = $derived(item.relinkCandidates.filter((source) => {
    const needle = query.trim().toLocaleLowerCase()
    return !needle || `${source.title} ${source.path} ${source.created ?? ''}`.toLocaleLowerCase().includes(needle)
  }))

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    const source = item.relinkCandidates.find((candidate) => candidate.path === selected)
    if (source) await onSubmit(source)
  }

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    queueMicrotask(() => sheetEl?.focus())
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) onCancel()
      if (event.key !== 'Tab' || !sheetEl) return
      const focusable = [...sheetEl.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)')]
      if (!focusable.length) return
      const first = focusable[0]
      const last = focusable.at(-1)!
      if (!focusable.includes(document.activeElement as HTMLElement)) {
        event.preventDefault()
        const destination = event.shiftKey ? last : first
        destination.focus()
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
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="relink-title" tabindex="-1" bind:this={sheetEl}>
    <header>
      <div><h2 id="relink-title">{t('relink.title')}</h2><p>{t(item.relinkMatch === 'created' ? 'relink.helpExact' : 'relink.helpManual')}</p></div>
      <button aria-label={t('common.cancel')} disabled={saving} onclick={onCancel}>×</button>
    </header>
    {#if item.relinkCandidates.length === 0}
      <div class="empty">{t('relink.noCandidates')}</div>
    {:else}
      <form onsubmit={submit}>
        <input class="search" bind:value={query} placeholder={t('search.placeholder')} />
        <div class="list">
          {#each candidates as source}
            <label class:selected={selected === source.path}>
              <input type="radio" name="source" value={source.path} bind:group={selected} />
              <span>
                <strong>{source.title}</strong>
                <small>{source.path}</small>
                <small>{t('relink.created')}: {source.created ?? t('relink.createdUnknown')}</small>
              </span>
            </label>
          {/each}
        </div>
        <footer>
          <button type="button" class="cancel" disabled={saving} onclick={onCancel}>{t('common.cancel')}</button>
          <button type="submit" class="submit" disabled={saving || !selected}>{t('action.relink')}</button>
        </footer>
      </form>
    {/if}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, #000 32%, transparent); backdrop-filter: blur(8px); }
  .sheet { width: min(580px, 100%); max-height: calc(100vh - 48px); overflow: hidden; display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: 20px; background: var(--sheet); color: var(--fg); box-shadow: 0 24px 80px color-mix(in srgb, #000 30%, transparent); }
  header { display: flex; justify-content: space-between; gap: 16px; padding: 22px 24px 16px; }
  h2 { margin: 0; font-size: 19px; }
  header p { margin: 6px 0 0; color: var(--muted); font-size: 12px; line-height: 1.5; }
  header button { flex: none; width: 28px; height: 28px; border: none; border-radius: 50%; background: var(--chip); color: var(--muted-strong); font-size: 20px; cursor: pointer; }
  form { min-height: 0; display: flex; flex-direction: column; gap: 12px; padding: 0 24px 22px; }
  .search { box-sizing: border-box; width: 100%; border: 1px solid var(--line-strong); border-radius: 9px; background: var(--input); color: var(--fg); padding: 9px 10px; font: inherit; }
  .list { min-height: 0; max-height: 340px; overflow: auto; display: grid; gap: 6px; }
  .list label { display: flex; align-items: center; gap: 10px; padding: 10px; border: 1px solid var(--line); border-radius: 10px; cursor: pointer; }
  .list label:hover { background: var(--hover); }
  .list label.selected { border-color: var(--accent); background: var(--accent-soft); }
  .list span, .list strong, .list small { display: block; min-width: 0; }
  .list span { overflow: hidden; }
  .list strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
  .list small { margin-top: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); }
  .empty { padding: 16px 24px 24px; color: var(--muted); }
  footer { display: flex; justify-content: flex-end; gap: 8px; }
  footer button { border-radius: 9px; padding: 8px 14px; font: inherit; font-weight: 650; cursor: pointer; }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .cancel { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .submit { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
</style>
