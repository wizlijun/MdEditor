<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import ChoiceField, { type ChoiceOption } from './ChoiceField.svelte'
  import type { PlaceInput } from '../lib/events'
  import type { SettlementExit } from '../lib/model'
  import type { WorkspaceItem } from '../lib/repository'
  import { t } from '../lib/strings'

  type Route = PlaceInput['route']
  type ExitKind = SettlementExit['kind']

  let {
    item,
    saving,
    initialRoute,
    onCancel,
    onSubmit,
  }: {
    item: WorkspaceItem
    saving: boolean
    initialRoute?: Route
    onCancel(): void
    onSubmit(input: PlaceInput): Promise<void>
  } = $props()

  const projection = untrack(() => item.projection)
  const inferredRoute: Route = projection?.state === 'waiting' ? 'wait' : projection?.state === 'dormant' ? 'park' : projection?.state === 'closed' ? 'settle' : 'commit'
  let route = $state<Route>(untrack(() => initialRoute ?? inferredRoute))
  let commitment = $state(projection?.state === 'wip' ? projection.commitment : '')
  let nextAction = $state(projection?.state === 'wip' || projection?.state === 'dormant' ? projection.next_action ?? '' : '')
  let closeCondition = $state(projection?.state === 'wip' ? projection.close_condition : '')
  let waitingFor = $state(projection?.state === 'waiting' ? projection.waiting_for : '')
  let reviewAt = $state(projection?.state === 'waiting' ? projection.review_at.slice(0, 10) : '')
  let wakeTrigger = $state(projection?.state === 'dormant' ? projection.wake_trigger : '')
  let exitKind = $state<ExitKind | ''>('')
  let exitVia = $state('')
  let reason = $state('')
  let target = $state('')
  let result = $state('')
  let invalid = $state(false)
  let sheetEl: HTMLDivElement | undefined = $state()

  function choice(label: string, value = label): ChoiceOption {
    return { label, value }
  }

  function dateAfter(days: number): string {
    const date = new Date()
    date.setHours(12, 0, 0, 0)
    date.setDate(date.getDate() + days)
    const pad = (value: number) => String(value).padStart(2, '0')
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
  }

  const commitmentOptions = $derived([
    choice(t('preset.commit.verify', { title: item.title })),
    choice(t('preset.commit.prototype', { title: item.title })),
    choice(t('preset.commit.plan', { title: item.title })),
    choice(t('preset.commit.deliver', { title: item.title })),
  ])
  const nextActionOptions = $derived([
    choice(t('preset.next.evidence')),
    choice(t('preset.next.experiment')),
    choice(t('preset.next.draft')),
    choice(t('preset.next.user')),
  ])
  const closeConditionOptions = $derived([
    choice(t('preset.close.decision')),
    choice(t('preset.close.prototype')),
    choice(t('preset.close.used')),
    choice(t('preset.close.metric')),
  ])
  const waitingOptions = $derived([
    choice(t('preset.wait.person', { title: item.title })),
    choice(t('preset.wait.agent', { title: item.title })),
    choice(t('preset.wait.review', { title: item.title })),
    choice(t('preset.wait.evidence', { title: item.title })),
  ])
  const reviewOptions = $derived([
    choice(t('preset.date.tomorrow'), dateAfter(1)),
    choice(t('preset.date.days3'), dateAfter(3)),
    choice(t('preset.date.week1'), dateAfter(7)),
    choice(t('preset.date.weeks2'), dateAfter(14)),
    choice(t('preset.date.month1'), dateAfter(30)),
  ])
  const wakeOptions = $derived([
    choice(t('preset.wake.week'), dateAfter(7)),
    choice(t('preset.wake.month'), dateAfter(30)),
    choice(t('preset.wake.related')),
    choice(t('preset.wake.repeat')),
    choice(t('preset.wake.evidence')),
  ])
  const reasonOptions = $derived([
    choice(t('preset.reason.value')),
    choice(t('preset.reason.timing')),
    choice(t('preset.reason.disproved')),
    choice(t('preset.reason.better')),
  ])
  const resultOptions = $derived([
    choice(t('preset.result.accepted')),
    choice(t('preset.result.source')),
    choice(t('preset.result.delivered')),
    choice(t('preset.result.recorded')),
  ])

  const viaOptions = $derived.by(() => {
    switch (exitKind) {
      case 'done': return ['', 'delegate']
      case 'stopped': return ['drop', 'disproved', 'ignore']
      case 'transferred': return ['merge', 'project', 'delegate', 'buy', 'publish']
      case 'compressed': return ['principle', 'automate']
      default: return []
    }
  })
  const reasonRequired = $derived(exitKind === 'stopped' && (exitVia === 'drop' || exitVia === 'disproved'))
  const targetRequired = $derived(exitKind === 'transferred' || exitKind === 'compressed')

  function chooseRoute(value: Route) {
    route = value
    invalid = false
  }

  function chooseExitKind(value: ExitKind) {
    exitKind = value
    exitVia = ''
    reason = ''
    target = ''
    result = ''
    invalid = false
  }

  function exitValue(): SettlementExit {
    if (!exitKind) throw new Error('an outcome must be selected')
    if (exitKind === 'done') return exitVia === 'delegate' ? { kind: 'done', via: 'delegate' } : { kind: 'done' }
    if (exitKind === 'stopped') return { kind: 'stopped', via: exitVia as 'drop' | 'disproved' | 'ignore' }
    if (exitKind === 'transferred') return { kind: 'transferred', via: exitVia as 'merge' | 'project' | 'delegate' | 'buy' | 'publish' }
    return { kind: 'compressed', via: exitVia as 'principle' | 'automate' }
  }

  function viaLabel(via: string): string {
    if (!via) return t('via.none')
    if (via === 'delegate') {
      return t(exitKind === 'done' ? 'via.delegateDone' : 'via.delegateTransferred')
    }
    return t(`via.${via}` as never)
  }

  function isValid(): boolean {
    if (route === 'commit') return Boolean(commitment.trim() && nextAction.trim() && closeCondition.trim())
    if (route === 'wait') return Boolean(waitingFor.trim() && reviewAt.trim())
    if (route === 'park') return Boolean(wakeTrigger.trim())
    if (!exitKind) return false
    if (exitKind !== 'done' && !exitVia) return false
    if (reasonRequired && !reason.trim()) return false
    if (targetRequired && !target.trim()) return false
    return true
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (!isValid()) {
      invalid = true
      return
    }
    invalid = false
    if (route === 'commit') {
      await onSubmit({ route, commitment, next_action: nextAction, close_condition: closeCondition })
    } else if (route === 'wait') {
      await onSubmit({ route, waiting_for: waitingFor, review_at: reviewAt })
    } else if (route === 'park') {
      await onSubmit({ route, wake_trigger: wakeTrigger, next_action: nextAction })
    } else if (exitKind === 'done') {
      await onSubmit({ route, exit: exitValue(), result })
    } else if (exitKind === 'stopped') {
      await onSubmit({ route, exit: exitValue(), reason })
    } else {
      await onSubmit({ route, exit: exitValue(), target })
    }
  }

  onMount(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    queueMicrotask(() => sheetEl?.focus())
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) onCancel()
      if (event.key !== 'Tab' || !sheetEl) return
      const focusable = [...sheetEl.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)')]
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
  <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="place-title" tabindex="-1" bind:this={sheetEl}>
    <header>
      <div>
        <p>{t('sheet.choose')}</p>
        <h2 id="place-title">{t('sheet.title', { title: item.title })}</h2>
      </div>
      <button class="close" aria-label={t('common.cancel')} disabled={saving} onclick={onCancel}>×</button>
    </header>

    <div class="routes" aria-label={t('sheet.choose')}>
      {#each [
        ['commit', 'route.commit', 'route.commit.detail'],
        ['wait', 'route.wait', 'route.wait.detail'],
        ['park', 'route.park', 'route.park.detail'],
        ['settle', 'route.settle', 'route.settle.detail'],
      ] as option}
        <button class:active={route === option[0]} aria-pressed={route === option[0]} onclick={() => chooseRoute(option[0] as Route)}>
          <strong>{t(option[1] as never)}</strong>
          <span>{t(option[2] as never)}</span>
        </button>
      {/each}
    </div>

    <form onsubmit={submit}>
      {#if route === 'commit'}
        <ChoiceField field="commitment" label={t('field.commitment')} bind:value={commitment} options={commitmentOptions} placeholder={t('field.commitment.placeholder')} multiline />
        <ChoiceField field="nextAction" label={t('field.nextAction')} bind:value={nextAction} options={nextActionOptions} placeholder={t('field.nextAction.placeholder')} multiline />
        <ChoiceField field="closeCondition" label={t('field.closeCondition')} bind:value={closeCondition} options={closeConditionOptions} placeholder={t('field.closeCondition.placeholder')} multiline />
      {:else if route === 'wait'}
        <ChoiceField field="waitingFor" label={t('field.waitingFor')} bind:value={waitingFor} options={waitingOptions} placeholder={t('field.waitingFor.placeholder')} multiline />
        <ChoiceField field="reviewAt" label={t('field.reviewAt')} bind:value={reviewAt} options={reviewOptions} type="date" />
      {:else if route === 'park'}
        <ChoiceField field="wakeTrigger" label={t('field.wakeTrigger')} bind:value={wakeTrigger} options={wakeOptions} placeholder={t('field.wakeTrigger.placeholder')} />
        <small class="help">{t('field.wakeTrigger.help')}</small>
        <ChoiceField field="nextAction" label={`${t('field.nextAction')} · ${t('common.optional')}`} bind:value={nextAction} options={nextActionOptions} placeholder={t('field.nextAction.placeholder')} multiline />
      {:else}
        <div class="split">
          <label>
            <span>{t('field.exitKind')}</span>
            <select value={exitKind} onchange={(event) => chooseExitKind(event.currentTarget.value as ExitKind)}>
              <option value="" disabled>{t('field.exitKind.placeholder')}</option>
              <option value="done">{t('exit.done')}</option>
              <option value="stopped">{t('exit.stopped')}</option>
              <option value="transferred">{t('exit.transferred')}</option>
              <option value="compressed">{t('exit.compressed')}</option>
            </select>
          </label>
          <label>
            <span>{t('field.exitVia')}</span>
            <select bind:value={exitVia} disabled={!exitKind}>
              {#if exitKind !== 'done'}<option value="" disabled>{t('field.exitVia.placeholder')}</option>{/if}
              {#each viaOptions as via}
                <option value={via}>{viaLabel(via)}</option>
              {/each}
            </select>
          </label>
        </div>
        {#if reasonRequired || exitKind === 'stopped'}
          <ChoiceField field="reason" label={`${t('field.reason')}${reasonRequired ? '' : ` · ${t('common.optional')}`}`} bind:value={reason} options={reasonOptions} placeholder={t('field.reason.placeholder')} multiline />
        {/if}
        {#if targetRequired}
          <label><span>{t('field.target')}</span><input bind:value={target} placeholder={t('field.target.placeholder')} /></label>
        {/if}
        {#if exitKind === 'done'}
          <ChoiceField field="result" label={`${t('field.result')} · ${t('common.optional')}`} bind:value={result} options={resultOptions} placeholder={t('field.result.placeholder')} />
        {/if}
      {/if}

      {#if invalid}<p class="error" role="alert">{t('error.required')}</p>{/if}
      <footer>
        <button type="button" class="cancel" disabled={saving} onclick={onCancel}>{t('common.cancel')}</button>
        <button type="submit" class="submit" disabled={saving}>{t('common.save')}</button>
      </footer>
    </form>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, #000 32%, transparent); backdrop-filter: blur(8px); }
  .sheet { width: min(680px, 100%); max-height: calc(100vh - 48px); overflow: auto; box-sizing: border-box; border: 1px solid var(--line); border-radius: 20px; background: var(--sheet); color: var(--fg); box-shadow: 0 24px 80px color-mix(in srgb, #000 30%, transparent); }
  header { display: flex; justify-content: space-between; gap: 16px; padding: 22px 24px 12px; }
  header p { margin: 0 0 4px; color: var(--muted); font-size: 12px; }
  h2 { margin: 0; font-size: 19px; line-height: 1.3; }
  .close { width: 28px; height: 28px; border: none; border-radius: 50%; background: var(--chip); color: var(--muted-strong); font-size: 20px; line-height: 1; cursor: pointer; }
  .routes { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; padding: 8px 24px 18px; }
  .routes button { min-height: 68px; padding: 10px 12px; text-align: left; border: 1px solid var(--line); border-radius: 12px; background: var(--card); color: var(--fg); cursor: pointer; }
  .routes button:hover { background: var(--hover); }
  .routes button.active { border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); background: var(--accent-soft); }
  .routes strong, .routes span { display: block; }
  .routes strong { font-size: 13px; }
  .routes span { margin-top: 4px; color: var(--muted); font-size: 11px; line-height: 1.35; }
  form { display: grid; gap: 13px; padding: 18px 24px 22px; border-top: 1px solid var(--line); }
  label { display: grid; gap: 6px; font-size: 12px; font-weight: 600; }
  input, select { width: 100%; box-sizing: border-box; border: 1px solid var(--line-strong); border-radius: 9px; background: var(--input); color: var(--fg); padding: 9px 10px; font: inherit; outline: none; }
  input:focus, select:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .split { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .error { margin: 0; color: var(--danger); font-size: 12px; }
  .help { margin-top: -7px; color: var(--muted); font-size: 11px; line-height: 1.45; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding-top: 4px; }
  footer button { border-radius: 9px; padding: 8px 14px; font: inherit; font-weight: 650; cursor: pointer; }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .cancel { border: 1px solid var(--line); background: transparent; color: var(--fg); }
  .submit { border: 1px solid var(--accent); background: var(--accent); color: #fff; }
  @media (max-width: 620px) { .routes, .split { grid-template-columns: 1fr; } }
</style>
