<!-- SignSheet.svelte — one-screen "sign the bet" modal (candidate → open).
     quoted: shows the quote, one tap to sign. nominated / manual: prediction
     input REQUIRED. Confidence = five stars, each anchored to a probability
     (v1.1 R2). Optional premortem (certainty wording, Klein) + alternatives.
     On submit calls doSign (agent origin, from a candidate) or doManualCreate
     (manual origin). created = today ISO. prediction/confidence/premortem/
     alternatives have NO edit entry after signing — set only here. -->
<script lang="ts">
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import type { NewCandidate } from '../lib/candidate'
  import type { Confidence, Trigger } from '../lib/model'
  import ConfidenceBar from './ConfidenceBar.svelte'
  import { doSign, doManualCreate } from '../lib/store.svelte'
  import { t } from '../lib/strings'

  let {
    candidate,
    consumeDate = null,
    onClose,
  }: {
    candidate?: NewCandidate | null
    // Source file date of the candidate; when set (agent origin), signing marks
    // the originating diary item accepted so the candidate stops reappearing.
    consumeDate?: string | null
    onClose: () => void
  } = $props()

  const today = new Date().toISOString().slice(0, 10)
  function plusDays(iso: string, n: number): string {
    return new Date(new Date(iso).getTime() + n * 86_400_000).toISOString().slice(0, 10)
  }

  const isQuoted = $derived(candidate?.prediction_source === 'quoted')

  // Seed fields ONCE from the candidate (a modal is opened fresh per candidate,
  // so a one-time snapshot is correct). Read via a plain snapshot so we don't
  // create reactive dependencies on the prop.
  // svelte-ignore state_referenced_locally
  const seed = candidate as NewCandidate | null | undefined
  let title = $state(seed?.title ?? '')
  let prediction = $state(seed?.prediction ?? (seed?.prediction_source === 'quoted' ? (seed?.quote ?? '') : ''))
  let confidence = $state<Confidence | null>(seed?.confidence ?? null)
  let checkDate = $state(seed?.check_date ?? plusDays(today, 14))
  let premortem = $state(seed?.premortem_hint ?? '')
  let alternativesText = $state(seed?.alternatives?.join('\n') ?? '')
  let triggerText = $state(seed?.triggers?.map((tr) => tr.if).join('\n') ?? '')
  let submitting = $state(false)
  let error = $state('')
  function close(): void { if (!submitting) onClose() }

  const canSubmit = $derived(
    !!title.trim() && !!prediction.trim() && !!confidence && !!checkDate && !submitting
  )

  function parseTriggers(): Trigger[] | undefined {
    const lines = triggerText.split('\n').map((l) => l.trim()).filter(Boolean)
    return lines.length ? lines.map((l) => ({ if: l })) : undefined
  }
  function parseAlternatives(): string[] | undefined {
    const lines = alternativesText.split('\n').map((l) => l.trim()).filter(Boolean)
    return lines.length ? lines : undefined
  }

  async function submit() {
    if (!canSubmit || !confidence) {
      if (!prediction.trim()) error = t('sign.predictionRequired')
      return
    }
    submitting = true
    error = ''
    const triggers = parseTriggers()
    const alternatives = parseAlternatives()
    const pm = premortem.trim()
    try {
      if (candidate) {
        await doSign(
          {
            title: title.trim(),
            prediction: prediction.trim(),
            confidence,
            checkDate,
            origin: 'agent',
            created: today,
            ...(candidate.source?.conv_id ? { source_conv: candidate.source.conv_id } : {}),
            ...(isQuoted && candidate.quote ? { quote: candidate.quote } : {}),
            ...(pm ? { premortem: pm } : {}),
            ...(alternatives ? { alternatives } : {}),
            ...(triggers ? { triggers } : {}),
            ...(candidate.state ? { state: candidate.state } : {}),
          },
          consumeDate ? { date: consumeDate, candidateId: candidate.id } : undefined,
        )
      } else {
        await doManualCreate({
          title: title.trim(),
          prediction: prediction.trim(),
          confidence,
          checkDate,
          created: today,
          ...(pm ? { premortem: pm } : {}),
          ...(alternatives ? { alternatives } : {}),
          ...(triggers ? { triggers } : {}),
        })
      }
      onClose()
    } catch (e) {
      error = String(e)
      submitting = false
    }
  }
</script>

<div class="overlay" onclick={close} role="presentation">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="sheet" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-busy={submitting} aria-labelledby="sign-title" tabindex="-1" use:modalFocus={{ onClose: close, canClose: () => !submitting }}>
    <h2 id="sign-title">{candidate ? t('sign.title') : t('sign.title.new')}</h2>

    <label class="field">
      <span class="lbl">{t('sign.titleLabel')}</span>
      <input class="title-input" bind:value={title} placeholder={t('sign.title.new')} />
    </label>

    {#if isQuoted && candidate?.quote}
      <p class="quoted-lead">{t('sign.quotedLead')}:</p>
      <blockquote class="quote">{candidate.quote}</blockquote>
    {/if}

    <label class="field">
      <span class="lbl">{t('sign.prediction')} <span class="req">*</span></span>
      <textarea
        bind:value={prediction}
        rows="2"
        placeholder={candidate && !isQuoted ? (candidate.prediction ?? t('sign.nominatedLead')) : t('sign.nominatedLead')}
      ></textarea>
    </label>

    <div class="field">
      <ConfidenceBar
        label={t('sign.confidenceLabel')}
        required
        value={confidence}
        onChange={(c) => (confidence = c)}
      />
    </div>

    <label class="field">
      <span class="lbl">{t('sign.checkDate')}</span>
      <input type="date" bind:value={checkDate} min={today} />
    </label>

    <!-- The three optional fields each carry a `why` line: on their own the
         labels read as jargon, and a user who can't see what the field buys
         them at review time just leaves it blank. -->
    <label class="field">
      <span class="lbl">{t('sign.premortem')}</span>
      <span class="why">{t('sign.premortemWhy')}</span>
      <textarea bind:value={premortem} rows="2" placeholder={t('sign.premortemHint')}></textarea>
    </label>

    <label class="field">
      <span class="lbl">{t('sign.alternatives')}</span>
      <span class="why">{t('sign.alternativesWhy')}</span>
      <textarea bind:value={alternativesText} rows="4" placeholder={t('sign.alternativesHint')}></textarea>
    </label>

    <label class="field">
      <span class="lbl">{t('sign.triggers')}</span>
      <span class="why">{t('sign.triggersWhy')}</span>
      <textarea bind:value={triggerText} rows="4" placeholder={t('sign.triggersHint')}></textarea>
    </label>

    {#if error}<p class="err" role="alert">{error}</p>{/if}

    <div class="actions">
      <button type="button" class="ghost" disabled={submitting} onclick={close}>{t('common.cancel')}</button>
      <button type="button" class="primary" disabled={!canSubmit} onclick={submit}>
        {t('sign.submit')}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed; inset: 0; background: rgba(0, 0, 0, 0.4);
    display: flex; align-items: center; justify-content: center; padding: 1rem; z-index: 50;
  }
  .sheet {
    background: var(--sheet-bg, Canvas); color: CanvasText;
    border-radius: 12px; padding: 1.25rem 1.5rem; width: min(480px, 100%);
    max-height: 90vh; overflow-y: auto; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 1rem; font-size: 1.1rem; }
  .field { display: block; margin-bottom: 0.9rem; }
  .lbl { display: block; font-size: 0.8rem; opacity: 0.7; margin-bottom: 0.35rem; }
  .why {
    display: block; font-size: 12px; line-height: 1.45;
    color: var(--ui-secondary); margin: -0.15rem 0 0.35rem;
  }
  .req { color: var(--ui-danger); }
  input, textarea {
    width: 100%; box-sizing: border-box; padding: 0.5rem; border: 1px solid var(--line, #d1d5db);
    border-radius: 6px; font: inherit; background: var(--input-bg, Field); color: FieldText;
  }
  textarea { resize: vertical; }
  .quoted-lead { margin: 0 0 0.25rem; font-size: 0.8rem; opacity: 0.7; }
  .quote { margin: 0 0 0.9rem; padding: 0.5rem 0.75rem; border-left: 3px solid var(--accent, #2563eb);
    background: color-mix(in srgb, currentColor 5%, transparent); border-radius: 0 6px 6px 0; }
  .err { color: var(--ui-danger); font-size: 0.85rem; margin: 0 0 0.5rem; }
  .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 0.5rem; }
  button.primary { padding: 0.5rem 1rem; border: 0; border-radius: 6px; background: var(--accent, #2563eb); color: #fff; cursor: pointer; }
  button.primary:disabled { opacity: 0.5; cursor: not-allowed; }
  button.ghost { padding: 0.5rem 1rem; border: 1px solid var(--line, #d1d5db); border-radius: 6px; background: transparent; color: inherit; cursor: pointer; }
</style>
