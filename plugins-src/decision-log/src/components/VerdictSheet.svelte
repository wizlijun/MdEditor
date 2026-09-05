<!-- VerdictSheet.svelte — one-screen verdict modal (open → archive).
     Q1 t('verdict.q1') → hit/partial/miss; Q2 t('verdict.q2') → yes/no
     (stillEndorse); Q3 (optional, only when Q2=No): weakest of the SDG six
     elements (v1.1 R5). Top shows read-only prediction (🔒) + evidence.
     Anti-anchoring (v1.1 R7): the AI's suggested_outcome NEVER pre-selects a
     button — only an explicit user click on a suggestion strip passes a preset.
     On submit calls doVerdict, resolved = today. Coach tone: a miss is NOT
     rendered as failure (spec §3 S5 / §7.4). -->
<script lang="ts">
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import type { OpenDecision, Outcome, WeakestElement } from '../lib/model'
  import type { Closure } from '../lib/candidate'
  import { OUTCOMES, WEAKEST_ELEMENTS } from '../lib/model'
  import ConfidenceBar from './ConfidenceBar.svelte'
  import { doVerdict } from '../lib/store.svelte'
  import { t } from '../lib/strings'

  let {
    decision,
    closure,
    presetOutcome,
    consumeDate,
    onClose,
  }: {
    decision: OpenDecision
    closure?: Closure | null
    /** 预填 Q1 结果(来自到期/关闭建议);closure.suggested_outcome 优先级更高。 */
    presetOutcome?: Outcome | null
    /** 若该裁决有来源建议(closure/edit),提交后把对应候选文件里的 closures 项标 accepted。 */
    consumeDate?: string | null
    onClose: () => void
  } = $props()

  const today = new Date().toISOString().slice(0, 10)

  // Anti-anchoring: only an explicit user action (suggestion-strip click) may
  // preset Q1; the closure's suggested_outcome is deliberately NOT used here.
  // svelte-ignore state_referenced_locally
  let outcome = $state<Outcome | null>(presetOutcome ?? null)
  let stillEndorse = $state<boolean | null>(null)
  let weakest = $state<WeakestElement | null>(null)
  let submitting = $state(false)
  let error = $state('')
  function close(): void { if (!submitting) onClose() }

  const evidence = $derived(closure?.evidence ?? [])
  // Progress notes appended over the decision's life (accepted "note"/progress
  // suggestions). Shown read-only in the detail sheet, newest last.
  const progress = $derived(decision.progress ?? [])
  const canSubmit = $derived(!!outcome && stillEndorse !== null && !submitting)

  async function submit() {
    if (!outcome || stillEndorse === null || submitting) return
    submitting = true
    error = ''
    try {
      await doVerdict(
        decision.id,
        {
          outcome,
          stillEndorse,
          resolved: today,
          ...(stillEndorse === false && weakest ? { weakestElement: weakest } : {}),
          ...(evidence.length ? { evidence } : {}),
        },
        consumeDate ? { date: consumeDate } : undefined,
      )
      onClose()
    } catch (e) {
      error = String(e)
      submitting = false
    }
  }
</script>

<div class="overlay" onclick={close} role="presentation">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="sheet" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-busy={submitting} aria-label={t('verdict.submit')} tabindex="-1" use:modalFocus={{ onClose: close, canClose: () => !submitting }}>
    <!-- read-only, locked context -->
    <div class="locked">
      <span class="lock" title={t('verdict.locked')}>🔒</span>
      <div>
        <div class="pred">{decision.prediction}</div>
        <div class="conf"><ConfidenceBar value={decision.confidence} readonly /></div>
      </div>
    </div>

    <div class="evidence">
      <span class="lbl">{t('verdict.evidence')}</span>
      {#if evidence.length}
        <ul>
          {#each evidence as ev, i (i)}
            <li>“{ev.quote}”</li>
          {/each}
        </ul>
      {:else}
        <p class="muted">{t('verdict.noEvidence')}</p>
      {/if}
    </div>

    {#if progress.length}
      <div class="progress">
        <span class="lbl">{t('sugg.progress')}</span>
        <ul>
          {#each progress as p, i (i)}
            <li><span class="date">{p.date}</span>{p.text}</li>
          {/each}
        </ul>
      </div>
    {/if}

    <!-- Q1 -->
    <div class="question">
      <p class="q">{t('verdict.q1')}</p>
      <div class="choices">
        {#each OUTCOMES as o (o)}
          <button
            type="button"
            class="choice"
            class:active={outcome === o}
            aria-pressed={outcome === o}
            onclick={() => (outcome = o)}
          >
            {o === 'hit' ? '✅' : o === 'partial' ? '◐' : '·'}
            {t(`verdict.${o === 'miss' ? 'miss' : o}` as 'verdict.hit')}
          </button>
        {/each}
      </div>
    </div>

    <!-- Q2 -->
    <div class="question">
      <p class="q">{t('verdict.q2')}</p>
      <div class="choices">
        <button type="button" class="choice" class:active={stillEndorse === true} aria-pressed={stillEndorse === true} onclick={() => (stillEndorse = true)}>
          {t('verdict.endorseYes')}
        </button>
        <button type="button" class="choice" class:active={stillEndorse === false} aria-pressed={stillEndorse === false} onclick={() => (stillEndorse = false)}>
          {t('verdict.endorseNo')}
        </button>
      </div>
    </div>

    <!-- Q3 (optional, only when Q2 = No): weakest link, SDG six elements -->
    {#if stillEndorse === false}
      <div class="question">
        <p class="q">{t('verdict.q3')} <span class="q3-hint">{t('verdict.q3Hint')}</span></p>
        <div class="elements">
          {#each WEAKEST_ELEMENTS as el (el)}
            <button
              type="button"
              class="el-chip"
              class:active={weakest === el}
              aria-pressed={weakest === el}
              onclick={() => (weakest = weakest === el ? null : el)}
            >
              {t(`el.${el}` as 'el.frame')}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    {#if error}<p class="err" role="alert">{error}</p>{/if}

    <div class="actions">
      <button type="button" class="ghost" disabled={submitting} onclick={close}>{t('common.cancel')}</button>
      <button type="button" class="primary" disabled={!canSubmit} onclick={submit}>
        {t('verdict.submit')}
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
  .locked {
    display: flex; gap: 0.6rem; align-items: flex-start;
    padding: 0.75rem; border-radius: 8px; margin-bottom: 1rem;
    background: color-mix(in srgb, currentColor 6%, transparent);
  }
  .lock { flex: 0 0 auto; }
  .pred { font-weight: 500; }
  .conf { font-size: 0.8rem; opacity: 0.65; margin-top: 0.2rem; }
  .evidence { margin-bottom: 1.1rem; }
  .lbl { display: block; font-size: 0.75rem; opacity: 0.6; margin-bottom: 0.3rem; }
  .evidence ul { margin: 0; padding-left: 1.1rem; font-size: 0.88rem; }
  .evidence li { margin: 0.15rem 0; }
  .muted { margin: 0; font-size: 0.85rem; opacity: 0.55; }
  .progress { margin-bottom: 1.1rem; }
  .progress ul { margin: 0; padding-left: 1.1rem; font-size: 0.88rem; list-style: none; }
  .progress li { margin: 0.25rem 0; line-height: 1.4; overflow-wrap: anywhere; }
  .progress .date { display: inline-block; margin-right: 0.5rem; font-size: 0.78rem; opacity: 0.55; font-variant-numeric: tabular-nums; }
  .question { margin-bottom: 1.1rem; }
  .q { margin: 0 0 0.5rem; font-size: 0.95rem; }
  .choices { display: flex; gap: 0.5rem; }
  .choice {
    flex: 1; padding: 0.55rem; border: 1px solid var(--line, #d1d5db); border-radius: 6px;
    background: transparent; color: inherit; font: inherit; cursor: pointer;
  }
  .choice.active { background: var(--accent, #2563eb); color: #fff; border-color: var(--accent, #2563eb); }
  .q3-hint { font-size: 0.78rem; opacity: 0.55; font-weight: 400; margin-left: 0.35rem; }
  .elements { display: flex; flex-wrap: wrap; gap: 0.4rem; }
  .el-chip {
    padding: 0.35rem 0.6rem; border: 1px solid var(--line, #d1d5db); border-radius: 999px;
    background: transparent; color: inherit; font: inherit; font-size: 0.82rem; cursor: pointer;
  }
  .el-chip.active { background: var(--accent, #2563eb); color: #fff; border-color: var(--accent, #2563eb); }
  .err { color: var(--ui-danger); font-size: 0.85rem; margin: 0 0 0.5rem; }
  .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 0.5rem; }
  button.primary { padding: 0.5rem 1rem; border: 0; border-radius: 6px; background: var(--accent, #2563eb); color: #fff; cursor: pointer; }
  button.primary:disabled { opacity: 0.5; cursor: not-allowed; }
  button.ghost { padding: 0.5rem 1rem; border: 1px solid var(--line, #d1d5db); border-radius: 6px; background: transparent; color: inherit; cursor: pointer; }
</style>
