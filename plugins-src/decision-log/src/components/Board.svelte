<!-- Board.svelte — three-column kanban (candidates → open → archive), each column
     renders Card. Free-lane drag: candidate → open opens SignSheet; open →
     archive opens VerdictSheet; archive → open reopens (light confirm →
     doReopen); every other drag (candidate→archive, open→candidate, same-column
     cross-cell) snaps back with a one-line note (no write). Dragged card dims
     (opacity .5); the drop-target column highlights. Clicking a card is the
     equivalent accessible path (opens the matching modal). Each Open card also
     shows pending agent suggestion strips (closures + edit_decisions) beneath it.
     Candidate column footer "+ New Decision" opens SignSheet (manual create). -->
<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { state as store, doReopen, doRejectCandidate, refresh, refreshIfChanged } from '../lib/store.svelte'
  import type { OpenDecision, Outcome } from '../lib/model'
  import type { NewCandidate, Closure } from '../lib/candidate'
  import Card from './Card.svelte'
  import SignSheet from './SignSheet.svelte'
  import VerdictSheet from './VerdictSheet.svelte'
  import ReviewPass from './ReviewPass.svelte'
  import SuggestionStrip, { type Suggestion } from './SuggestionStrip.svelte'
  import { t } from '../lib/strings'

  type Column = 'candidates' | 'open' | 'archive'

  // ── due check (weekly review) ──
  // Overdue-and-undecided open decisions: check-date <= today. Verdicts + strike
  // counting ride inside this review pass, never as a standalone interruption.
  const today = new Date().toISOString().slice(0, 10)
  const overdue = $derived(store.open.filter((d) => d['check-date'] <= today))
  let reviewQueue = $state<OpenDecision[] | null>(null)
  function startReview() {
    // snapshot the queue at open time; ReviewPass walks it by index
    if (overdue.length) reviewQueue = [...overdue]
  }

  // Flattened candidate list across all daily files, each tagged with its source
  // file date (_fileDate) so signing/rejecting can consume the right diary item.
  const candidates = $derived(
    store.candidates.flatMap((f) => f.new_candidates.map((c) => ({ ...c, _fileDate: f.fileDate })))
  )
  // First closure per decision id (with its source file date), so open cards can
  // prefill their verdict + consume the closure on submit.
  const closureById = $derived.by(() => {
    const m = new Map<string, { closure: Closure; date: string }>()
    for (const f of store.candidates)
      for (const c of f.closures) if (!m.has(c.decision_id)) m.set(c.decision_id, { closure: c, date: f.fileDate })
    return m
  })
  // Pending agent suggestions grouped by decision id (closures + edit_decisions),
  // each tagged with its source file date (from filename) for later consume/dismiss.
  // Closures are deduplicated: only the first pending closure per decision_id is kept
  // (consistent with closureById; avoids cross-day duplicate strips becoming orphans).
  // edit_decisions are not deduplicated: different days may carry distinct progress notes.
  const suggestionsById = $derived.by(() => {
    const m = new Map<string, Suggestion[]>()
    const seenClosures = new Set<string>()
    const push = (id: string, s: Suggestion) => {
      const arr = m.get(id)
      if (arr) arr.push(s)
      else m.set(id, [s])
    }
    for (const f of store.candidates) {
      for (const c of f.closures) {
        if (seenClosures.has(c.decision_id)) continue
        seenClosures.add(c.decision_id)
        push(c.decision_id, { kind: 'closure', date: f.fileDate, closure: c })
      }
      for (const e of f.edit_decisions) push(e.decision_id, { kind: 'edit', date: f.fileDate, edit: e })
    }
    return m
  })

  // ── modal state ──
  let signCandidate = $state<NewCandidate | null | undefined>(undefined) // undefined = closed; null = manual
  let signConsumeDate = $state<string | null>(null) // source file date of the candidate (consume on sign)
  let verdictDecision = $state<OpenDecision | null>(null)
  let verdictPreset = $state<Outcome | null>(null)
  let verdictConsumeDate = $state<string | null>(null)
  const signOpen = $derived(signCandidate !== undefined)

  // Open the verdict sheet for a decision, optionally prefilled from a suggestion.
  function openVerdictWith(decisionId: string, preset: Outcome | null, consumeDate: string | null) {
    const dec = store.open.find((x) => x.id === decisionId)
    if (!dec) return
    verdictPreset = preset
    verdictConsumeDate = consumeDate
    verdictDecision = dec
  }
  function closeVerdict() {
    verdictDecision = null
    verdictPreset = null
    verdictConsumeDate = null
  }

  // ── drag state (pointer-based) ──
  // HTML5 DnD is unusable inside the Tauri plugin window: the OS-level
  // drag-drop handler (dragDropEnabled) swallows page dragstart/drop, so the
  // board runs its own pointer-driven lane drag. dragging/hoverCol stay
  // reactive so the board can dim the dragged card and highlight the target.
  const DRAG_THRESHOLD = 5 // px of pointer travel before a press becomes a drag
  let dragging = $state<{ column: Column; id: string } | null>(null)
  let hoverCol = $state<Column | null>(null)
  let ghostTitle = $state('') // label shown in the follow-cursor ghost
  let ghostX = $state(0)
  let ghostY = $state(0)
  let invalidNote = $state('')
  let noteTimer: ReturnType<typeof setTimeout> | null = null

  // pending press (before threshold) — not yet a drag
  let press: { column: Column; id: string; title: string; startX: number; startY: number; pointerId: number } | null =
    null

  function flashInvalid() {
    invalidNote = t('drag.invalid')
    if (noteTimer) clearTimeout(noteTimer)
    noteTimer = setTimeout(() => (invalidNote = ''), 2200)
  }

  // Card pointerdown → arm a press. We do NOT preventDefault/capture here so a
  // plain click (no movement past threshold) still fires the card's onclick.
  function onCardPointerDown(column: Column, id: string, title: string, e: PointerEvent) {
    press = { column, id, title, startX: e.clientX, startY: e.clientY, pointerId: e.pointerId }
    ghostX = e.clientX
    ghostY = e.clientY
  }

  // Which column contains the given viewport point (or null if outside all).
  function colAtPoint(x: number, y: number): Column | null {
    for (const [col, el] of Object.entries(colEls) as [Column, HTMLElement | undefined][]) {
      const r = el?.getBoundingClientRect()
      if (r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return col
    }
    return null
  }

  function onWindowPointerMove(e: PointerEvent) {
    if (press && !dragging) {
      // promote press → drag once past the movement threshold
      const dx = e.clientX - press.startX
      const dy = e.clientY - press.startY
      if (Math.hypot(dx, dy) >= DRAG_THRESHOLD) {
        dragging = { column: press.column, id: press.id }
        ghostTitle = press.title
      }
    }
    if (!dragging) return
    ghostX = e.clientX
    ghostY = e.clientY
    const col = colAtPoint(e.clientX, e.clientY)
    hoverCol = col && col !== dragging.column ? col : null
  }

  function onWindowPointerUp() {
    const d = dragging
    const target = hoverCol
    cancelDrag()
    if (!d || !target || target === d.column) return
    // Free-lane drag: only the three legal forward/back transitions act.
    if (d.column === 'candidates' && target === 'open') {
      openSignFor(d.id)
    } else if (d.column === 'open' && target === 'archive') {
      openVerdictWith(d.id, null, null)
    } else if (d.column === 'archive' && target === 'open') {
      confirmReopen(d.id)
    } else {
      // candidate→archive, open→candidate = illegal.
      flashInvalid()
    }
  }

  // Abort any in-flight press/drag without running a transition.
  function cancelDrag() {
    press = null
    dragging = null
    hoverCol = null
    ghostTitle = ''
  }

  function onWindowKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape' && (dragging || press)) cancelDrag()
  }

  // column container refs, keyed by column, for hit-testing during pointer drag
  const colEls: { candidates?: HTMLElement; open?: HTMLElement; archive?: HTMLElement } = {}

  async function confirmReopen(archivedId: string) {
    if (typeof confirm === 'function' && !confirm(t('drag.reopenConfirm'))) return
    await doReopen(archivedId)
  }

  // Open the sign sheet for a candidate, carrying its source file date so signing
  // consumes (marks accepted → hides) the originating diary item.
  function openSignFor(candidateId: string) {
    const c = candidates.find((x) => x.id === candidateId)
    if (c) { signConsumeDate = c._fileDate ?? null; signCandidate = c }
  }

  // Mark a candidate inaccurate: record it negatively (so the agent avoids it)
  // and hide it. Uses the candidate's own source file date.
  async function rejectCandidate(c: NewCandidate & { _fileDate?: string }) {
    await doRejectCandidate(c, c._fileDate ?? '')
  }

  // ── refresh (manual + auto) ──
  // The host has no file-watch push API, so the board polls the vault and also
  // refreshes on window focus / tab visibility. Background refreshes only
  // reassign state when data actually changed (refreshIfChanged) → no flicker.
  let refreshing = $state(false)
  let autoInFlight = false // reentrancy guard for the background path

  async function doManualRefresh() {
    if (refreshing) return
    refreshing = true
    try {
      await refresh()
    } catch (e) {
      console.error('[decision-log] manual refresh failed:', e)
    } finally {
      refreshing = false
    }
  }

  // Background tick. Skips when hidden (no point polling an unfocused window),
  // when any modal is open or a lane drag is in flight (never interrupt the
  // user mid-action), while a manual refresh runs, or if a prior auto refresh
  // hasn't returned yet (reentrancy).
  async function maybeAuto() {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return
    if (signOpen || verdictDecision || reviewQueue) return
    if (dragging || refreshing || autoInFlight) return
    autoInFlight = true
    try {
      await refreshIfChanged()
    } catch (e) {
      console.error('[decision-log] auto refresh failed:', e)
    } finally {
      autoInFlight = false
    }
  }

  function onVis() {
    if (typeof document !== 'undefined' && document.visibilityState === 'visible') void maybeAuto()
  }
  function onFocus() { void maybeAuto() }

  let autoTimer: ReturnType<typeof setInterval> | null = null
  onMount(() => {
    autoTimer = setInterval(() => void maybeAuto(), 4000)
    window.addEventListener('focus', onFocus)
    document.addEventListener('visibilitychange', onVis)
  })
  onDestroy(() => {
    if (autoTimer) clearInterval(autoTimer)
    window.removeEventListener('focus', onFocus)
    document.removeEventListener('visibilitychange', onVis)
  })

  // Prefill the verdict sheet from the decision's closure (evidence + outcome +
  // consume date), if any. Suggestion strips pass their own preset/date directly.
  const verdictClosureEntry = $derived(
    verdictDecision ? (closureById.get(verdictDecision.id) ?? null) : null
  )
  const verdictClosure = $derived(verdictClosureEntry?.closure ?? null)
  // If the sheet wasn't opened from a suggestion strip, fall back to the closure's date.
  const effectiveConsumeDate = $derived(verdictConsumeDate ?? verdictClosureEntry?.date ?? null)
</script>

<svelte:window
  onpointermove={onWindowPointerMove}
  onpointerup={onWindowPointerUp}
  onpointercancel={cancelDrag}
  onkeydown={onWindowKeyDown}
/>

<div class="board-wrap" class:dragging-active={dragging}>
  <div class="toolbar">
    {#if overdue.length}
      <button type="button" class="review-btn" onclick={startReview}>
        {t('review.start')}
        <span class="badge">{overdue.length}</span>
      </button>
    {/if}
    <button
      type="button"
      class="refresh-btn"
      class:spinning={refreshing}
      title={t('refresh.hint')}
      disabled={refreshing}
      onclick={doManualRefresh}
    >
      <span class="refresh-icon" aria-hidden="true">⟳</span>
      {t('refresh')}
    </button>
  </div>

  <div class="board">
  <!-- Candidates -->
  <section
    bind:this={colEls.candidates}
    class="col"
    class:drop-target={hoverCol === 'candidates'}
    role="list"
  >
    <header class="col-head">{t('col.candidates')}</header>
    <div class="col-body">
      {#if candidates.length === 0}
        <p class="empty">{t('col.candidatesEmpty')}</p>
      {:else}
        <!-- Keys carry an index suffix: ids come from agent-written vault files
             and CAN collide (two decisions once shared 2026-07-31-03); a bare
             (id) key makes Svelte throw each_key_duplicate and abort the whole
             render — the window then hangs on "Loading…" forever. -->
        {#each candidates as c, ci (`${c._fileDate}/${c.id}@${ci}`)}
          <div class="card-slot" class:dragging={dragging?.column === 'candidates' && dragging?.id === c.id}>
            <Card
              column="candidates"
              candidate={c}
              onclick={() => openSignFor(c.id)}
              onDragStart={(e) => onCardPointerDown('candidates', c.id, c.title, e)}
              onReject={() => rejectCandidate(c)}
            />
          </div>
        {/each}
      {/if}
    </div>
    <button type="button" class="new-btn" onclick={() => (signCandidate = null)}>
      + {t('card.new')}
    </button>
  </section>

  <!-- Open -->
  <section
    bind:this={colEls.open}
    class="col"
    class:drop-target={hoverCol === 'open'}
    role="list"
  >
    <header class="col-head">{t('col.open')}</header>
    <div class="col-body">
      {#if store.open.length === 0}
        <p class="empty">{t('col.openEmpty')}</p>
      {:else}
        {#each store.open as d, di (`${d.id}@${di}`)}
          <div class="card-slot" class:dragging={dragging?.column === 'open' && dragging?.id === d.id}>
            <Card
              column="open"
              decision={d}
              onclick={() => openVerdictWith(d.id, null, null)}
              onDragStart={(e) => onCardPointerDown('open', d.id, d.title, e)}
            />
            {#each suggestionsById.get(d.id) ?? [] as s, i (i)}
              <SuggestionStrip
                suggestion={s}
                onVerdict={(preset, date) => openVerdictWith(d.id, preset, date)}
              />
            {/each}
          </div>
        {/each}
      {/if}
    </div>
  </section>

  <!-- Archive (drag back to Open to reopen) -->
  <section
    bind:this={colEls.archive}
    class="col archive-col"
    class:drop-target={hoverCol === 'archive'}
    role="list"
  >
    <header class="col-head">{t('col.archive')}</header>
    <div class="col-body">
      {#if store.archived.length === 0}
        <p class="empty">{t('col.archiveEmpty')}</p>
      {:else}
        {#each store.archived as a, ai (`${a.id}@${ai}`)}
          <div class="card-slot" class:dragging={dragging?.column === 'archive' && dragging?.id === a.id}>
            <Card
              column="archive"
              decision={a}
              onDragStart={(e) => onCardPointerDown('archive', a.id, a.id, e)}
            />
          </div>
        {/each}
      {/if}
    </div>
  </section>
  </div>
</div>

{#if dragging}
  <div class="drag-ghost" style="left:{ghostX}px; top:{ghostY}px">{ghostTitle}</div>
{/if}

{#if invalidNote}
  <div class="drag-note" role="status">{invalidNote}</div>
{/if}

{#if signOpen}
  <SignSheet
    candidate={signCandidate}
    consumeDate={signConsumeDate}
    onClose={() => { signCandidate = undefined; signConsumeDate = null }}
  />
{/if}
{#if verdictDecision}
  <VerdictSheet
    decision={verdictDecision}
    closure={verdictClosure}
    presetOutcome={verdictPreset}
    consumeDate={effectiveConsumeDate}
    onClose={closeVerdict}
  />
{/if}
{#if reviewQueue}
  <ReviewPass queue={reviewQueue} onClose={() => { reviewQueue = null }} />
{/if}

<style>
  .board-wrap {
    min-width: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  @media (max-width: 660px) { .board-wrap { flex: none; height: 480px; } }
  /* While a lane drag is in flight the whole board shows the grabbing cursor. */
  .board-wrap.dragging-active,
  .board-wrap.dragging-active :global(.card) { cursor: grabbing; }
  .toolbar {
    flex: 0 0 auto;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem 0;
  }
  .refresh-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.4rem 0.8rem;
    border: 1px solid var(--line, #d1d5db);
    border-radius: 999px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    opacity: 0.85;
  }
  .refresh-btn:hover:not(:disabled) { opacity: 1; background: var(--ui-hover); }
  .refresh-btn:disabled { cursor: default; opacity: 0.55; }
  .refresh-icon { display: inline-block; line-height: 1; }
  .refresh-btn.spinning .refresh-icon { animation: dl-spin 0.8s linear infinite; }
  @keyframes dl-spin { to { transform: rotate(360deg); } }
  .review-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.9rem;
    border: 1px solid var(--accent, #2563eb);
    border-radius: 999px;
    background: var(--accent, #2563eb);
    color: #fff;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .review-btn:hover { opacity: 0.9; }
  .review-btn .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 1.3rem;
    height: 1.3rem;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.25);
    font-size: 0.78rem;
    font-weight: 600;
  }
  .board {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(3, minmax(160px, 1fr));
    gap: 0.75rem;
    padding: 1rem;
    min-height: 0;
    overflow-x: auto;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 10px;
    background: color-mix(in srgb, currentColor 2%, transparent);
  }
  .col-head {
    padding: 0.6rem 0.8rem;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.65;
    border-bottom: 1px solid var(--line, #e5e7eb);
  }
  .col.drop-target {
    border-color: var(--accent, #2563eb);
    background: color-mix(in srgb, var(--accent, #2563eb) 6%, transparent);
  }
  .col-body {
    flex: 1;
    overflow-y: auto;
    padding: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .card-slot { display: flex; flex-direction: column; gap: 0.35rem; }
  .card-slot.dragging { opacity: 0.5; }
  /* Follow-cursor ghost of the dragged card (pointer-based lane drag). */
  .drag-ghost {
    position: fixed;
    transform: translate(0.6rem, 0.6rem);
    max-width: 16rem;
    padding: 0.5rem 0.7rem;
    border: 1px solid var(--accent, #2563eb);
    border-radius: 8px;
    background: var(--card-bg, color-mix(in srgb, currentColor 6%, transparent));
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.18);
    font-size: 0.9rem;
    line-height: 1.35;
    font-weight: 500;
    opacity: 0.85;
    pointer-events: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    z-index: 60;
  }
  .empty { margin: 0.5rem 0; font-size: 13px; color: var(--ui-secondary); line-height: 1.4; }
  .new-btn {
    margin: 0 0.6rem 0.6rem;
    padding: 0.5rem;
    border: 1px dashed var(--line, #d1d5db);
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    opacity: 0.8;
  }
  .new-btn:hover { opacity: 1; border-color: var(--accent, #2563eb); }
  .archive-col { opacity: 0.95; }
  .drag-note {
    position: fixed;
    bottom: 1rem;
    left: 50%;
    transform: translateX(-50%);
    padding: 0.5rem 1rem;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.8);
    color: #fff;
    font-size: 0.85rem;
    z-index: 40;
  }
</style>
