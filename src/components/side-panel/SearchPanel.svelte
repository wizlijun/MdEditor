<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import { listen } from '@tauri-apps/api/event'
  import type { Tab } from '../../lib/tabs.svelte'
  import { openFile } from '../../lib/tabs.svelte'
  import { requestReveal } from '../../lib/outline/reveal.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import { showError } from '../../lib/dialogs'
  import { isImeKey } from '../../lib/ime'
  import { setSideVisible } from '../../lib/side-panel/registry.svelte'
  import SideViewSwitcher from './SideViewSwitcher.svelte'
  import { searchStore, isIndexNotReady } from '../../lib/search/store.svelte'
  import { DEFAULT_LIMIT, type SearchHit } from '../../lib/search/api'
  import { decideTrigger, DEEP_AFTER_MS, DEEP_TIMEOUT_MS } from '../../lib/search/input-trigger'
  import { groupHits, type HitGroup, type FileGroup } from '../../lib/search/grouping'
  import { parseHighlightTerms, previewLine, highlightParts } from '../../lib/search/preview'
  import { openSettings } from '../../lib/ui-state.svelte'

  // `tab` is part of every side view's props contract (see SidePanel.svelte);
  // this panel is vault-wide rather than per-document, so only the
  // `SideViewSwitcher` below reads it.
  let { tab }: { tab: Tab | null } = $props()

  // Local input value — kept separate from `searchStore.query` so keystrokes
  // are never lost to the debounce window (typing updates this immediately;
  // the store lags behind on purpose).
  let inputValue = $state('')
  // The query THIS panel last asked the store to run (set in the same
  // synchronous span as the `searchStore.run()` call that issues it — see
  // `runShallow`/`runDeep` below). Lets the sync effect further down tell
  // "the store finally caught up with what I typed" (ordinary debounce lag,
  // must NOT be reflected back into `inputValue` — that would erase
  // whatever the user has typed since) apart from "something ELSE changed
  // `searchStore.query` out from under this panel" (an external caller —
  // e.g. the settings page's "Unlabeled" tier row, design spec §7.4 — MUST
  // be reflected, or the user lands on real results with an empty input box
  // and their very next keystroke silently drops the filter that got them
  // there — review round 2, Minor 9).
  let lastIssuedQuery = ''
  let debounceTimer: ReturnType<typeof setTimeout> | undefined
  // The auto-escalation to a deep scan, armed only after a fast query came
  // back empty and stayed empty for a beat.
  let deepTimer: ReturnType<typeof setTimeout> | undefined
  // True between `compositionstart` and `compositionend` — i.e. while an IME
  // is showing candidates. Everything typed in that window is a pinyin/kana
  // buffer, not a query.
  let composing = $state(false)
  onDestroy(cancelTimers)

  function cancelTimers() {
    if (debounceTimer) clearTimeout(debounceTimer)
    if (deepTimer) clearTimeout(deepTimer)
    debounceTimer = undefined
    deepTimer = undefined
  }

  // Anything other than the known "not ready" case is shown as-is — better
  // than nothing, not pretending to translate arbitrary Rust error text.
  let errorText = $derived(
    isIndexNotReady(searchStore.error) ? t('search.notReady') : searchStore.error,
  )

  // Grouping (task B-T7, design spec §5) — UI-only: `groupHits` is a pure
  // function with no Svelte import (see `src/lib/search/grouping.ts`), this
  // is the one place its output is rendered. `notemd search`'s own default
  // output is untouched by this — it stays flat `path:line:text`.
  let groups = $derived(groupHits(searchStore.hits))

  // Named-type group headers use the raw `concept_type` string verbatim
  // (it's an open, plugin-extensible vocabulary — see `CONCEPT_TYPE` in
  // `src/lib/okf/concept.ts` — so translating it here would mean every new
  // type silently falls back to its own name anyway); the two poles and the
  // catch-all use translated labels.
  function groupLabel(group: HitGroup): string {
    switch (group.kind) {
      case 'pinned': return t('search.group.pinned')
      case 'human': return t('search.group.human')
      case 'source': return t('search.group.source')
      case 'unlabeled': return t('search.group.unlabeled')
      case 'derivedOther': return t('search.group.other')
      case 'derivedType': return group.conceptType ?? ''
    }
  }

  function groupKey(group: HitGroup): string {
    return group.kind === 'derivedType' ? `derivedType:${group.conceptType}` : group.kind
  }

  // Which file rows are open. Keyed by group *and* path: the same file can
  // appear under two groups (a note with both a human hit and a source hit),
  // and expanding one must not expand the other.
  const expanded = new SvelteSet<string>()
  function fileKey(group: HitGroup, file: FileGroup): string {
    return `${groupKey(group)}\u0000${file.path}`
  }
  function toggleFile(group: HitGroup, file: FileGroup) {
    const key = fileKey(group, file)
    if (!expanded.delete(key)) expanded.add(key)
  }

  // A new query invalidates every open row — otherwise last query's expansions
  // land on whichever same-named files happen to come back this time. `untrack`
  // keeps the write off this effect's own dependency list; without it the
  // `SvelteSet` mutation re-invalidates the effect that just ran.
  $effect(() => {
    searchStore.query
    untrack(() => expanded.clear())
  })

  // The words to mark up. Parsed from the query the same way the backend
  // parses it, so `path:` and friends never highlight anything in the prose.
  let terms = $derived(parseHighlightTerms(searchStore.query))

  // A hit's one displayable line — `hit.text` is a whole block, and in a vault
  // full of ```json / ```mermaid fences that block is routinely forty lines of
  // punctuation. See `src/lib/search/preview.ts`.
  function preview(hit: SearchHit) {
    return previewLine(hit.text, terms)
  }

  // Every keystroke re-decides *when* (and whether) to query — see
  // `input-trigger.ts` for the three rules. Rescheduling also cancels a
  // pending deep scan: the user typing on is the clearest possible signal
  // that the previous query is no longer the question.
  function scheduleSearch() {
    cancelTimers()
    const d = decideTrigger(inputValue, composing)
    if (d.kind === 'hold') return
    if (d.kind === 'clear') { lastIssuedQuery = ''; searchStore.clear(); return }
    debounceTimer = setTimeout(() => { void runShallow() }, d.delayMs)
  }

  // The fast tier: index only. A miss here is cheap and, crucially, does not
  // block the next keystroke behind a full-vault scan.
  async function runShallow() {
    const asked = inputValue
    lastIssuedQuery = asked
    await searchStore.run(asked, { deep: false })
    // Fast tier missed and a scan would look further. Offer it via the hint,
    // and — for the user who is sitting there waiting rather than reading the
    // hint — take it ourselves after a pause, under a time budget.
    if (searchStore.deepAvailable && inputValue === asked) {
      deepTimer = setTimeout(() => { void runDeep() }, DEEP_AFTER_MS)
    }
  }

  function runDeep() {
    cancelTimers()
    lastIssuedQuery = inputValue
    return searchStore.run(inputValue, { deep: true, timeoutMs: DEEP_TIMEOUT_MS })
  }

  // The count cap is invisible from the response (`total` counts what came
  // back, not what exists), so "exactly a full page" is the honest tell that
  // there may be more. A query with exactly DEFAULT_LIMIT real hits re-runs
  // once and comes back identical — harmless, and the offer disappears.
  let maybeCapped = $derived(
    !searchStore.lastAll && searchStore.hits.length >= DEFAULT_LIMIT,
  )

  // Re-run the visible query with the cap lifted, at the tier the visible
  // results came from — upgrading shallow→deep here would silently change
  // WHAT is being searched, not just how much of it is shown.
  function runAll() {
    cancelTimers()
    lastIssuedQuery = inputValue
    return searchStore.run(
      inputValue,
      searchStore.lastDeep
        ? { deep: true, timeoutMs: DEEP_TIMEOUT_MS, all: true }
        : { deep: false, all: true },
    )
  }

  // Review round 2, Minor 9: reflects an externally-issued `searchStore.run()`
  // (bypassing this panel's own input entirely) back into `inputValue`.
  // Guarded on `lastIssuedQuery` rather than firing whenever
  // `searchStore.query !== inputValue` — during the debounce window that
  // inequality is also true for perfectly ordinary lag (the user has typed
  // ahead of what the store has caught up to), and reflecting THAT back
  // would erase live keystrokes, exactly the bug `inputValue` was split out
  // to prevent in the first place. `lastIssuedQuery` only ever changes in
  // the same synchronous span as this panel's own `searchStore.run()` call
  // (`runShallow`/`runDeep`/the clear paths above), so it tracks
  // `searchStore.query` in lockstep for everything THIS panel issues — a
  // mismatch can only mean someone else changed it.
  $effect(() => {
    if (searchStore.query !== lastIssuedQuery) {
      inputValue = searchStore.query
      lastIssuedQuery = searchStore.query
    }
  })

  function onKeydown(e: KeyboardEvent) {
    // A Return that closes an IME candidate window belongs to the IME, not to
    // us. `isImeKey` is that distinction plus the two fallbacks old webviews
    // need (see src/lib/ime.ts) — the editors share it.
    if (isImeKey(e)) return
    if (e.key === 'Enter') {
      void runDeep()
    } else if (e.key === 'Escape') {
      cancelTimers()
      inputValue = ''
      lastIssuedQuery = ''
      searchStore.clear()
    }
  }

  function onInput(e: Event) {
    // Belt and braces with the `composing` flag: Safari/WebKit has been known
    // to deliver an `input` before `compositionstart` for the first key of a
    // composition, and `isComposing` is correct on the event itself.
    if ((e as InputEvent).isComposing) { cancelTimers(); return }
    scheduleSearch()
  }

  function onCompositionEnd() {
    composing = false
    // The committed characters only exist now, so this is the first moment
    // the input is a real query.
    scheduleSearch()
  }

  // NOTE: no `onRebuild` here on purpose. This panel's rebuild button was
  // replaced by the gear that opens the "Index & Search" settings tab, which
  // owns rebuilding (with confirmation, live progress and the skipped-files
  // list). The query-responsiveness rework still had the button, and its
  // post-rebuild re-query lives on in `refreshAfterIndexUpdate` below, which
  // re-runs at the tier the visible results came from whenever
  // `search://index-updated` fires — including the one this backend rebuild
  // emits when it finishes.
  async function onOpenHit(hit: SearchHit) {
    try {
      await openFile(hit.absPath)
      // Jump to the line the panel actually showed, not to the top of the
      // block: `hit.line` is where the block starts, and `p.line` is how far
      // into it the match sits.
      //
      // The anchor is the *cleaned* text on purpose. Rich mode finds its
      // target by scanning rendered text nodes, where the markup is already
      // gone — a raw `**外**骨骼` would never match. Source mode prefers the
      // line number anyway and only falls back to the anchor.
      const p = previewLine(hit.text, terms)
      requestReveal(hit.line + p.line, p.text || hit.text, hit.absPath)
    } catch (e) {
      showError(String(e))
    }
  }

  // Reindex on save elsewhere in the vault; if a query is active, silently
  // refresh it so newly-searchable content shows up without the user
  // re-typing.
  //
  // Coalescing is done on *our own* refreshes (`refreshing`/`refreshPending`),
  // not by skipping whenever `searchStore.loading` is set. That earlier guard
  // was described as "purely to avoid pointless calls, not correctness", and
  // that was wrong: an index-updated event arriving while any query was in
  // flight — including one the user just typed — was dropped outright, with
  // nothing scheduled to retry it, leaving results that silently predate the
  // update until the user typed again. Overlapping with a user query is
  // harmless: `SearchStore.run` stamps a monotonic sequence and discards
  // every response but the newest, and the refresh is issued last, so the
  // fresher answer is the one that lands.
  let refreshing = false
  let refreshPending = false
  async function refreshAfterIndexUpdate() {
    if (!searchStore.query.trim()) return
    if (refreshing) { refreshPending = true; return }
    refreshing = true
    try {
      do {
        refreshPending = false
        // Re-run at the tier the visible results came from, or a shallow
        // refresh would silently downgrade a deep answer to "no matches".
        await searchStore.run(searchStore.query, {
          // Same principle as the tier: refresh at the limit the visible
          // results came from, or an index update would silently snap an
          // uncapped answer back to the first 50.
          all: searchStore.lastAll,
          ...(searchStore.lastDeep ? { deep: true, timeoutMs: DEEP_TIMEOUT_MS } : { deep: false }),
        })
      } while (refreshPending && searchStore.query.trim())
    } finally {
      refreshing = false
    }
  }

  // KNOWN GAP: this panel has no local signal for a rebuild in progress,
  // from ANYWHERE — another window, a concurrent CLI invocation, or this
  // app's own "搜索与索引" Settings tab (the only in-app rebuild trigger now
  // that this panel's button is gone). `notemd_search_rebuild` holds the
  // backend index lock for the rebuild's full duration; if this event fires
  // while any of those holds it, the rerun below just blocks for that whole
  // duration with only the generic `loading` state to show for it — a silent
  // hang, not a crash, but indistinguishable from one to the user. Note this
  // is NOT covered by SettingsDialog's own busy state: `indexStatus.progress`
  // is a store that tab subscribes to for its own UI, and this panel doesn't
  // read it. A real fix means either wiring this panel to `indexStatus` too
  // (extra subscription + an initial poll for a rebuild already in flight —
  // see `IndexStatusStore.refresh()`'s doc comment for why the poll is
  // needed) or a backend-reported "busy" signal; deliberately not done here
  // since it would re-add the input-disabling behavior this task's brief
  // explicitly removed from this panel, which is a product call for the
  // plan's owner, not a side effect of a comment fix.
  $effect(() => {
    const pending = listen('search://index-updated', () => { void refreshAfterIndexUpdate() })
    return () => { void pending.then((un) => un()) }
  })
</script>

<div class="search-content">
  <header>
    <SideViewSwitcher side="left" {tab} />
    <button class="hbtn settings-btn" title={t('search.openIndexSettings')} aria-label={t('search.openIndexSettings')} onclick={() => openSettings('search')}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    </button>
    <!-- Collapse sits last, at the right edge — same place the folder view
         puts it, so the button doesn't move when you switch views. -->
    <button class="hbtn" title={t('search.hide')} aria-label={t('search.hide')} onclick={() => void setSideVisible('left', false)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2" />
        <line x1="9" y1="3" x2="9" y2="21" />
        <polyline points="16 15 13 12 16 9" />
      </svg>
    </button>
  </header>

  <div class="search-input-row">
    <input
      type="text"
      class="search-input"
      placeholder={t('search.placeholder')}
      bind:value={inputValue}
      oninput={onInput}
      onkeydown={onKeydown}
      oncompositionstart={() => { composing = true; cancelTimers() }}
      oncompositionend={onCompositionEnd}
    />
  </div>

  <div class="body">
    {#if searchStore.error}
      <div class="error-row">
        <p class="error">{errorText}</p>
      </div>
    {:else if searchStore.loading}
      <p class="empty">{searchStore.lastDeep ? t('search.deepRunning') : '…'}</p>
    {:else if searchStore.deepAvailable}
      <!-- Not the same as "no matches": the index missed, and a slower scan
           has not looked yet. Saying "no matches" here would be a lie the
           user has no way to see through. -->
      <button class="deep-hint" onclick={() => void runDeep()}>{t('search.deepHint')}</button>
    {:else if searchStore.route !== null && searchStore.hits.length === 0}
      <p class="empty">{t('search.noResults')}</p>
    {:else if searchStore.hits.length > 0}
      {#each groups as group (groupKey(group))}
        <div class="group">
          <div class="group-header">
            <span class="group-label">{groupLabel(group)}</span>
            <span class="group-count">{t('search.group.count', { n: group.hitCount })}</span>
          </div>
          <ul class="files">
            {#each group.files as file (file.path)}
              {@const single = file.hits.length === 1}
              {@const open = expanded.has(fileKey(group, file))}
              {@const head = preview(file.hits[0])}
              <li class="file">
                <!-- One button for the whole collapsed card. A single-hit file
                     never expands — the expansion would show the very line
                     already previewed — so it opens the hit directly and is
                     not an expandable control at all. -->
                <button
                  class="file-row"
                  class:open
                  title={file.path}
                  aria-expanded={single ? undefined : open}
                  onclick={() => (single ? void onOpenHit(file.hits[0]) : toggleFile(group, file))}
                >
                  <span class="file-head">
                    <span class="twisty" class:hidden={single} aria-hidden="true"></span>
                    <span class="file-name">{file.name}</span>
                    <span class="file-count">{file.hits.length}</span>
                  </span>
                  {#if !open && head.text}
                    <!-- Kept on one line on purpose: Svelte preserves the
                         newline+indent between tags as a text node, which
                         would show up as a leading space in the preview. -->
                    <span class="preview">{#if head.lang}<span class="lang">{head.lang}</span>{/if}{#each highlightParts(head.text, terms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span>
                  {/if}
                </button>

                {#if open}
                  <ul class="hits">
                    {#each file.hits as hit, i (hit.path + ':' + hit.line + ':' + i)}
                      {@const p = preview(hit)}
                      <li class="hit">
                        <button class="row" onclick={() => void onOpenHit(hit)}>
                          {#if hit.breadcrumb}
                            <span class="breadcrumb" title={hit.breadcrumb}>{hit.breadcrumb}</span>
                          {/if}
                          <!-- Same one-line rule as the preview above: the
                               leading spans carry their own margins, and any
                               newline between them renders as extra space. -->
                          <span class="text"><span class="loc">{hit.line + p.line}</span>{#if hit.agentBy}<span class="marker" title={t('search.agentWritten', { agent: hit.agentBy })}>✦</span>{/if}{#if hit.humanVerified}<span class="marker" title={t('search.humanVerified')}>●</span>{/if}{#if p.lang}<span class="lang">{p.lang}</span>{/if}{#each highlightParts(p.text, terms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span>
                        </button>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}
      {#if maybeCapped}
        <button class="show-all" onclick={() => void runAll()}>{t('search.showAll')}</button>
      {/if}
    {/if}
  </div>

  {#if !searchStore.error}
    <footer class="status">
      {#if searchStore.route !== null}
        <span>{t('search.resultCount', { n: searchStore.total, ms: searchStore.tookMs })}</span>
        {#if searchStore.route === 't1-scan'}
          <span class="fallback">{t('search.fallbackScan')}</span>
        {/if}
        {#if searchStore.truncated}
          <span class="fallback">{t('search.partial')}</span>
        {/if}
      {/if}
    </footer>
  {/if}
</div>

<style>
  .search-content {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--drawer-bg, #f6f6f6);
  }
  header {
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid var(--border-color, #3333);
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .hbtn {
    display: inline-flex; align-items: center; justify-content: center;
    border: 0; background: transparent; cursor: pointer;
    padding: 3px; border-radius: 4px; opacity: 0.7;
  }
  .hbtn svg { display: block; }
  .hbtn:hover:not(:disabled) { background: rgba(0,0,0,0.08); opacity: 1; }
  .search-input-row {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border-color, #3333);
  }
  .search-input {
    width: 100%; box-sizing: border-box;
    border: 1px solid rgba(0,0,0,0.15); border-radius: 5px;
    padding: 4px 8px; font: inherit; font-size: 12px;
    background: var(--input-bg, #fff); color: inherit;
  }
  .search-input:focus { outline: none; border-color: rgba(0,120,255,0.6); }
  .body { flex: 1; overflow-y: auto; padding: 4px; }
  .empty { padding: 8px; opacity: 0.5; font-size: 12px; }
  .deep-hint {
    display: block; width: 100%; text-align: left;
    padding: 8px; font: inherit; font-size: 12px;
    border: 0; background: transparent; color: inherit;
    opacity: 0.62; cursor: pointer; border-radius: 6px;
  }
  .deep-hint:hover { background: rgba(0,0,0,0.05); opacity: 0.85; }
  /* Same voice as .deep-hint — both are "there may be more" offers. */
  .show-all {
    display: block; width: 100%; text-align: center;
    margin-top: 4px; padding: 6px 8px; font: inherit; font-size: 12px;
    border: 0; background: transparent; color: inherit;
    opacity: 0.62; cursor: pointer; border-radius: 6px;
  }
  .show-all:hover { background: rgba(0,0,0,0.05); opacity: 0.85; }
  .error-row { padding: 8px; display: flex; flex-direction: column; gap: 6px; }
  .error { margin: 0; font-size: 12px; color: #c0392b; }
  .group + .group { margin-top: 6px; }
  .group-header {
    display: flex; align-items: baseline; gap: 6px;
    padding: 4px 8px 2px;
    font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.02em;
    opacity: 0.55;
  }
  .group-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .group-count { opacity: 0.7; font-weight: 400; text-transform: none; }
  /* The whole result tree is three levels deep but only ever indents once, by
     8px, and expresses the last step with a rule rather than whitespace — the
     panel is narrow enough that a second indent level would cost more than the
     hierarchy is worth. */
  .files { list-style: none; margin: 0; padding: 0; }
  .file { border-radius: 6px; }
  .file-row {
    display: flex; flex-direction: column; gap: 1px;
    width: 100%; text-align: left;
    border: 0; background: transparent; color: inherit; cursor: pointer;
    font: inherit; padding: 4px 8px; border-radius: 6px;
  }
  .file-row:hover { background: rgba(0,0,0,0.05); }
  .file-head { display: flex; align-items: center; gap: 4px; min-width: 0; }
  /* A CSS triangle, not a glyph: buttons do not inherit font-size, so a
     character here would size unpredictably against the app's text scale. */
  .twisty { position: relative; flex: 0 0 10px; height: 10px; }
  .twisty::before {
    content: ''; position: absolute; left: 2px; top: 1px;
    border-left: 5px solid currentColor;
    border-top: 4px solid transparent;
    border-bottom: 4px solid transparent;
    opacity: 0.5;
    transform-origin: 2.5px 4px;
    transition: transform 0.12s ease;
  }
  .file-row.open .twisty::before { transform: rotate(90deg); }
  .twisty.hidden { visibility: hidden; }
  .file-name {
    font-size: 12px; font-weight: 600; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .file-count { margin-left: auto; padding-left: 6px; flex: 0 0 auto; font-size: 11px; opacity: 0.45; }
  .preview {
    display: block; font-size: 12px; opacity: 0.65;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .hits {
    list-style: none; margin: 0 0 2px 8px; padding: 0;
    border-left: 1px solid rgba(0,0,0,0.12);
  }
  .hit { border-radius: 4px; }
  .row {
    display: flex; flex-direction: column; gap: 1px;
    width: 100%; text-align: left;
    border: 0; background: transparent; color: inherit; cursor: pointer;
    font: inherit; padding: 3px 6px; border-radius: 4px;
  }
  .row:hover { background: rgba(0,0,0,0.05); }
  .breadcrumb {
    font-size: 10px; opacity: 0.45;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .text {
    font-size: 12px;
    display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .marker { opacity: 0.7; margin-right: 3px; }
  .loc { font-size: 10px; opacity: 0.45; margin-right: 4px; font-variant-numeric: tabular-nums; }
  /* Fence language, so a hit inside ```json reads as code rather than as
     broken prose. */
  .lang {
    font-size: 10px; opacity: 0.6; margin-right: 4px;
    padding: 0 3px; border-radius: 3px; white-space: nowrap;
    background: rgba(0,0,0,0.07);
  }
  mark {
    background: rgba(255,214,0,0.45); color: inherit;
    border-radius: 2px; padding: 0 1px;
  }
  .status {
    padding: 5px 10px; font-size: 11px; opacity: 0.6;
    border-top: 1px solid var(--border-color, #3333);
    display: flex; gap: 8px; align-items: center;
    flex-wrap: wrap;
  }
  .fallback { opacity: 0.8; }
  @media (prefers-color-scheme: dark) {
    .search-content { background: var(--drawer-bg, #1c1c1e); }
    .hbtn:hover:not(:disabled) { background: rgba(255,255,255,0.1); }
    .search-input { border-color: rgba(255,255,255,0.18); background: var(--input-bg, #2a2a2c); }
    .row:hover, .file-row:hover { background: rgba(255,255,255,0.08); }
    .hits { border-left-color: rgba(255,255,255,0.16); }
    .lang { background: rgba(255,255,255,0.12); }
    mark { background: rgba(255,214,0,0.3); }
    .deep-hint:hover { background: rgba(255,255,255,0.08); }
    .show-all:hover { background: rgba(255,255,255,0.08); }
    .error { color: #ff6b5e; }
  }
</style>
