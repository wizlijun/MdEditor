<!-- SuggestionStrip.svelte — a single pending agent suggestion rendered under an
     Open card. Coach tone, non-punitive accent (💡). One tap accepts / notes;
     [Details] expands the summary + evidence quotes for a second look; [×]
     dismisses (marks the source diary item dismissed).

     A suggestion is either a Closure (due/trigger → verdict) or an EditDecision
     (note/progress/adjust-check-date/close-*/drop). close-* and closures both
     need a still-endorse answer, so their [Accept] opens VerdictSheet prefilled
     (via onVerdict) rather than writing directly. Everything else calls the
     store action inline. -->
<script module lang="ts">
  import type { Closure, EditDecision } from '../lib/candidate'

  export type Suggestion =
    | { kind: 'closure'; date: string; closure: Closure }
    | { kind: 'edit'; date: string; edit: EditDecision }
</script>

<script lang="ts">
  import type { Outcome, Evidence } from '../lib/model'
  import { doAcceptEdit, doDismissEdit, doDismissClosure, doRejectClosure, doRejectEdit } from '../lib/store.svelte'
  import { t } from '../lib/strings'

  let {
    suggestion,
    onVerdict,
  }: {
    suggestion: Suggestion
    /** Open VerdictSheet prefilled for suggestions that need a still-endorse answer.
     *  preset = suggested outcome; date = source file date (for consume). */
    onVerdict: (preset: Outcome, date: string) => void
  } = $props()

  let expanded = $state(false)
  let busy = $state(false)

  // ── derive label + which action path this suggestion takes ──
  type Path = 'verdict' | 'accept'
  const outcomeMap: Record<string, Outcome> = {
    'close-hit': 'hit', 'close-partial': 'partial', 'close-miss': 'miss',
  }

  const label = $derived.by(() => {
    if (suggestion.kind === 'closure') {
      const o = suggestion.closure.suggested_outcome
      if (o === 'hit') return t('sugg.due.hit')
      if (o === 'partial') return t('sugg.due.partial')
      if (o === 'miss') return t('sugg.due.miss')
      return t('sugg.dueVerdict')
    }
    const e = suggestion.edit
    switch (e.suggested_action) {
      case 'note': return `${t('sugg.progress')}: ${e.summary}`
      case 'adjust-check-date': return `${t('sugg.adjustDate')} ${e.new_check_date ?? ''}`
      case 'close-hit': return t('sugg.closeHit')
      case 'close-partial': return t('sugg.closePartial')
      case 'close-miss': return t('sugg.closeMiss')
      case 'drop': return t('sugg.drop')
    }
  })

  const path = $derived.by<Path>(() => {
    if (suggestion.kind === 'closure') return 'verdict'
    const a = suggestion.edit.suggested_action
    return a === 'close-hit' || a === 'close-partial' || a === 'close-miss' ? 'verdict' : 'accept'
  })

  // note/progress edits get a softer "Note it" label; the rest say "Accept".
  const acceptLabel = $derived(
    suggestion.kind === 'edit' && suggestion.edit.suggested_action === 'note'
      ? t('sugg.note')
      : t('sugg.accept'),
  )

  const summary = $derived(suggestion.kind === 'edit' ? suggestion.edit.summary : '')
  const evidence = $derived<Evidence[]>(
    suggestion.kind === 'closure' ? (suggestion.closure.evidence ?? []) : (suggestion.edit.evidence ?? []),
  )

  async function accept() {
    if (busy) return
    if (path === 'verdict') {
      const preset: Outcome =
        suggestion.kind === 'closure'
          ? (suggestion.closure.suggested_outcome ?? 'partial')
          : outcomeMap[suggestion.edit.suggested_action]
      onVerdict(preset, suggestion.date)
      return
    }
    // accept path: only reachable for edit suggestions
    if (suggestion.kind !== 'edit') return
    busy = true
    try {
      await doAcceptEdit(suggestion.edit, suggestion.date)
    } catch (e) {
      console.error('[decision-log] accept suggestion failed:', e)
      busy = false
    }
  }

  async function dismiss() {
    if (busy) return
    busy = true
    try {
      if (suggestion.kind === 'closure') {
        await doDismissClosure(suggestion.closure.decision_id, suggestion.date)
      } else {
        await doDismissEdit(suggestion.edit, suggestion.date)
      }
    } catch (e) {
      console.error('[decision-log] dismiss suggestion failed:', e)
      busy = false
    }
  }

  // Mark inaccurate: record the suggestion negatively (so the agent avoids the
  // pattern next time) and hide it. Distinct from a plain dismiss.
  async function reject() {
    if (busy) return
    busy = true
    try {
      if (suggestion.kind === 'closure') {
        await doRejectClosure(suggestion.closure.decision_id, suggestion.date)
      } else {
        await doRejectEdit(suggestion.edit, suggestion.date)
      }
    } catch (e) {
      console.error('[decision-log] reject suggestion failed:', e)
      busy = false
    }
  }
</script>

<div class="strip">
  <div class="head">
    <span class="bulb">💡</span>
    <span class="label">{label}</span>
  </div>
  <div class="btns">
    <button type="button" class="accept" disabled={busy} onclick={accept}>{acceptLabel}</button>
    <button type="button" class="link" onclick={() => (expanded = !expanded)}>{t('sugg.detail')}</button>
    <button type="button" class="reject" title={t('reject.hint')} disabled={busy} onclick={reject}>{t('reject')}</button>
    <button type="button" class="x" title={t('sugg.dismiss')} disabled={busy} onclick={dismiss}>×</button>
  </div>
  {#if expanded}
    <div class="detail">
      {#if summary}<p class="summary">{summary}</p>{/if}
      {#if evidence.length}
        <span class="ev-lbl">{t('sugg.evidence')}</span>
        <ul>
          {#each evidence as ev, i (i)}
            <li>“{ev.quote}”</li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<style>
  .strip {
    border: 1px solid color-mix(in srgb, var(--accent, #2563eb) 35%, transparent);
    border-radius: 7px;
    background: color-mix(in srgb, var(--accent, #2563eb) 8%, transparent);
    padding: 0.4rem 0.5rem;
    font-size: 0.82rem;
  }
  .head { display: flex; align-items: flex-start; gap: 0.4rem; }
  .bulb { flex: 0 0 auto; line-height: 1.3; }
  .label { flex: 1; min-width: 0; line-height: 1.3; overflow-wrap: anywhere; }
  /* Actions sit on their own row beneath the label so a long localized label
     never collides with / squeezes the four buttons (zh strings are wider). */
  .btns {
    display: flex; align-items: center; justify-content: flex-end;
    flex-wrap: wrap; gap: 0.3rem; margin-top: 0.35rem;
  }
  .accept {
    padding: 0.2rem 0.5rem; border: 0; border-radius: 5px;
    background: var(--accent, #2563eb); color: #fff; font: inherit; font-size: 0.78rem; cursor: pointer;
  }
  .accept:disabled { opacity: 0.5; cursor: not-allowed; }
  .link {
    padding: 0.2rem 0.3rem; border: 0; background: transparent; color: inherit;
    font: inherit; font-size: 0.76rem; opacity: 0.7; cursor: pointer; text-decoration: underline;
  }
  .link:hover { opacity: 1; }
  /* Mark inaccurate: quiet by default, warms to red on hover to signal removal. */
  .reject {
    padding: 0.2rem 0.3rem; border: 0; background: transparent; color: inherit;
    font: inherit; font-size: 0.76rem; opacity: 0.55; cursor: pointer;
  }
  .reject:hover { opacity: 1; color: var(--ui-danger); }
  .reject:disabled { opacity: 0.3; cursor: not-allowed; }
  .x {
    padding: 0 0.35rem; border: 0; background: transparent; color: inherit;
    font-size: 1rem; line-height: 1; opacity: 0.55; cursor: pointer;
  }
  .x:hover { opacity: 1; }
  .x:disabled { opacity: 0.3; cursor: not-allowed; }
  .detail { margin-top: 0.4rem; padding-top: 0.4rem; border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent); }
  .summary { margin: 0 0 0.35rem; line-height: 1.4; }
  .ev-lbl { display: block; font-size: 12px; opacity: 0.6; margin-bottom: 0.2rem; }
  .detail ul { margin: 0; padding-left: 1rem; }
  .detail li { margin: 0.1rem 0; line-height: 1.35; }
</style>
