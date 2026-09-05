<!-- ReviewPass.svelte — the "due check" (weekly-review) focused overlay.
     Verdicts + skip handling ride inside this review pass, never as a
     standalone interruption (spec §4/§7/§S7/§S8). Overdue-and-undecided
     decisions are surfaced one at a time: give a verdict (VerdictSheet →
     doVerdict) or skip WITH a reason (v1.1 R4): not-yet → check-date +14d,
     no strike; irrelevant → drop, no guilt; avoid → strike (3rd downgrades).
     Closing the whole pass (Esc) counts nothing — a lapse is not avoidance
     (design review §1.5). A downgrade shows a coach-tone line, never a red
     failure. The done screen gently mentions sunk (downgraded) items. -->
<script lang="ts">
  import { modalFocus } from '../../../../src/lib/ui/modal-focus'
  import type { OpenDecision, SkipReason } from '../lib/model'
  import { state as store, doSkip, refresh } from '../lib/store.svelte'
  import VerdictSheet from './VerdictSheet.svelte'
  import Card from './Card.svelte'
  import { t } from '../lib/strings'

  let {
    queue,
    onClose,
  }: {
    // snapshot taken when the pass opens; we walk it by index so the board
    // mutating underneath us doesn't shift the queue.
    queue: OpenDecision[]
    onClose: () => Promise<void> | void
  } = $props()

  const today = new Date().toISOString().slice(0, 10)

  let index = $state(0)
  let showVerdict = $state(false)
  let askReason = $state(false) // skip pressed → show the three reasons
  let downgradedNote = $state('') // coach-tone line shown after an avoid-skip triggers a downgrade
  let busy = $state(false)
  let error = $state('')

  const current = $derived(queue[index] ?? null)
  const sunkCount = $derived(store.archived.filter((a) => a.status === 'downgraded').length)

  async function advance() {
    downgradedNote = ''
    showVerdict = false
    askReason = false
    if (index + 1 >= queue.length) {
      // queue done → close & refresh the board + scoreboard
      await refresh()
      await onClose()
      return
    }
    index += 1
  }

  async function onVerdictClosed() {
    // VerdictSheet only calls onClose after a successful doVerdict (or cancel);
    // either way we move on to the next card. On cancel the card stays overdue
    // and will resurface next due check.
    await advance()
  }

  async function skipWith(reason: SkipReason) {
    if (!current || busy) return
    busy = true
    error = ''
    try {
      const downgraded = await doSkip(current.id, reason, today)
      if (downgraded) {
        // passive self-cleanup, coach tone — pause on the line before advancing
        downgradedNote = t('review.downgraded')
        askReason = false
        busy = false
        return
      }
    } catch (e) {
      error = String(e)
      busy = false
      return
    }
    busy = false
    await advance()
  }
</script>

<div class="overlay" role="presentation">
  <div class="pass" role="dialog" aria-modal="true" aria-busy={busy} aria-label={t('review.title')} tabindex="-1" use:modalFocus={{ onClose: () => void onClose(), canClose: () => !showVerdict && !busy }}>
    <header class="pass-head">
      <span class="ttl">{t('review.title')}</span>
      <span class="progress">{index + 1} {t('review.of')} {queue.length}</span>
    </header>
    {#if error}<p class="error" role="alert">{error}</p>{/if}

    {#if current}
      <div class="focus">
        <Card column="open" decision={current} />
      </div>

      {#if downgradedNote}
        <p class="coach" role="status">{downgradedNote}</p>
        <div class="actions">
          <button type="button" class="primary" onclick={advance} aria-label={t('review.continue')}>→</button>
        </div>
      {:else if askReason}
        <p class="skip-q">{t('skip.q')}</p>
        <div class="reasons">
          <button type="button" class="reason" disabled={busy} onclick={() => skipWith('not-yet')}>{t('skip.notYet')}</button>
          <button type="button" class="reason" disabled={busy} onclick={() => skipWith('irrelevant')}>{t('skip.irrelevant')}</button>
          <button type="button" class="reason" disabled={busy} onclick={() => skipWith('avoid')}>{t('skip.avoid')}</button>
        </div>
        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={() => (askReason = false)}>{t('common.cancel')}</button>
        </div>
      {:else}
        {#if index + 1 >= queue.length && sunkCount > 0}
          <p class="sunk">⬇ {sunkCount} {t('review.sunk')}</p>
        {/if}
        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={() => (askReason = true)}>{t('review.skip')}</button>
          <button type="button" class="primary" disabled={busy} onclick={() => (showVerdict = true)}>{t('review.decide')}</button>
        </div>
      {/if}
    {/if}
  </div>
</div>

{#if showVerdict && current}
  <VerdictSheet decision={current} onClose={onVerdictClosed} />
{/if}

<style>
  .error { color: var(--ui-danger); overflow-wrap: anywhere; }
  .overlay {
    position: fixed; inset: 0; background: rgba(0, 0, 0, 0.5);
    display: flex; align-items: center; justify-content: center; padding: 1rem; z-index: 45;
  }
  .pass {
    background: var(--sheet-bg, Canvas); color: CanvasText;
    border-radius: 12px; padding: 1.25rem 1.5rem; width: min(460px, 100%);
    max-height: 90vh; overflow-y: auto; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
    display: flex; flex-direction: column; gap: 1rem;
  }
  .pass-head {
    display: flex; align-items: baseline; justify-content: space-between;
  }
  .ttl { font-size: 1rem; font-weight: 600; }
  .progress { font-size: 0.82rem; opacity: 0.6; }
  .focus { display: flex; flex-direction: column; }
  .coach {
    margin: 0; padding: 0.7rem 0.85rem; border-radius: 8px; font-size: 0.9rem; line-height: 1.5;
    background: color-mix(in srgb, currentColor 6%, transparent);
  }
  .skip-q { margin: 0; font-size: 0.9rem; opacity: 0.8; }
  .reasons { display: flex; flex-direction: column; gap: 0.4rem; }
  .reason {
    padding: 0.55rem 0.8rem; border: 1px solid var(--line, #d1d5db); border-radius: 8px;
    background: transparent; color: inherit; font: inherit; font-size: 0.9rem; cursor: pointer;
    text-align: left;
  }
  .reason:hover:not(:disabled) { border-color: var(--accent, #2563eb); }
  .reason:disabled { opacity: 0.5; cursor: default; }
  .sunk { margin: 0; font-size: 0.82rem; opacity: 0.6; }
  .actions { display: flex; justify-content: flex-end; gap: 0.5rem; }
  button.primary { padding: 0.5rem 1rem; border: 0; border-radius: 6px; background: var(--accent, #2563eb); color: #fff; cursor: pointer; }
  button.primary:disabled { opacity: 0.5; cursor: not-allowed; }
  button.ghost { padding: 0.5rem 1rem; border: 1px solid var(--line, #d1d5db); border-radius: 6px; background: transparent; color: inherit; cursor: pointer; }
  button.ghost:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
