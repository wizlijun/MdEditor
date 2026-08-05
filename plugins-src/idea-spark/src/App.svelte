<!-- App.svelte — the Idea Spark window: a blank page you write on.

     Layout (design §1: "open it and write"). No title bar at all — the window
     chrome is Tauri's job — so the editor starts at the very top edge and runs
     down to a single 38px action bar:

       ┌───────────────────────┬─────────┐
       │              ┌──────┐ │ inbox   │  ← ModeToggle, floating (absolute)
       │ editor       │👁│</>│ │ 240px,  │
       │ (flex:1)     └──────┘ │ optional│  ← InboxPanel; the inbox button
       ├───────────────────────┴─────────┤     toggles it and the choice is
       │ saved 19:42  New  Delegate  ▤  ⚙│  ← 38px bar    persisted (`inboxOpen`)
       └─────────────────────────────────┘

     EVERY pictograph in that sketch — 👁 </> ▤ ⚙ — is a stand-in for an SVG,
     not a character that appears in the markup. The bar's four icons come from
     `components/Icon.svelte` (New and Delegate are icon + label; the two on the
     right are icon-only), and the ModeToggle's two come from its own file.
     Nothing here is an emoji any more: a macOS emoji is a color bitmap, so it
     ignored both `currentColor` and the theme and sat at a different visual
     weight than the 2px strokes of the ModeToggle right above it.

     The inbox *squeezes* the editor rather than covering it (design §5), and is
     hidden by default: the window opens on a blank page, not on a file list.

     There is no save button: the document is written 1.5s after the user stops
     typing (`autosave.ts`), and forced to disk at every point where the buffer
     is about to be replaced or lost. The bar's left side reports that instead.

     Declares `color-scheme: light dark` so this standalone plugin window follows
     the system appearance (project convention — otherwise Canvas system colors
     get pinned to light; see MEMORY reference_webview_color_scheme).

     Startup (all of it in `onMount`, never in an `$effect` — an effect that
     synchronously calls store functions which read and write `$state` freezes
     the window, MEMORY feedback_svelte_effect_untrack):
       1. `boot()` — vault info → `.notemd/idea-spark.json` → directory listing.
          No vault ⇒ `needVault` and nothing else renders.
       2. `tick()` so the (now un-blocked) editor container is in the DOM.
       3. dynamically import the host's Editor Kit and mount it. If that throws
          (host too old, `editor.kit` capability missing, 404) the window does
          NOT go blank: it shows `editorUnavailable` and swaps in a plain
          textarea holding the very same markdown.
     The kit owns the theme (it fetches and watches the host's compiled CSS) —
     nothing here touches it. -->
<script lang="ts">
  import { onMount, tick } from 'svelte'
  import Celebration from './components/Celebration.svelte'
  import Icon from './components/Icon.svelte'
  import InboxPanel from './components/InboxPanel.svelte'
  import ModeToggle from './components/ModeToggle.svelte'
  import SettingsPopover from './components/SettingsPopover.svelte'
  import { delegateIdea, interpretStatus, POLL_MS, TASK_ID, type RunView } from './lib/agent-client'
  import { createAutosave } from './lib/autosave'
  import { agentStatus, bridge } from './lib/bridge'
  import { loadKit, type KitEditor, type KitMode } from './lib/editor-kit'
  import { pickPlaceholder, placeholderLines } from './lib/placeholder'
  import {
    boot,
    deleteIdea,
    finishRun,
    isBlank,
    loadIdea,
    markEdited,
    markPending,
    needsSaveBefore,
    newIdea,
    persist,
    rebaseline,
    reconcilePending,
    relPath,
    renameIdea,
    runInFlight,
    runStatusWord,
    saveIdea,
    showInEditor,
    state as store,
    titleOf,
    toast,
    toggleInbox,
  } from './lib/store.svelte'
  import { setLocale, t } from './lib/strings'

  setLocale(bridge().locale)

  let editorEl: HTMLDivElement | undefined = $state()
  let kit: KitEditor | null = null
  /** Editor content while the kit is unavailable (the degraded textarea). */
  let fallbackText = $state('')
  let mode = $state<KitMode>('rich')
  let settingsOpen = $state(false)
  /** A `host.agent.run` call is in flight (between the click and the run id). */
  let delegating = $state(false)
  /** claude-agent isn't installed/enabled — the layer pointing at the market. */
  let agentMissing = $state(false)
  /** Newest progress line of a watched run, with the idea it belongs to. Only
   *  rendered when that idea is the one in the editor. */
  let runProgress = $state<{ ideaRel: string; last: string } | null>(null)

  /** The run id arguing the OPEN document, or null. Derived, never assigned:
   *  `pending` is the single source of truth and the badge in the inbox reads
   *  the very same map (`statusOf`). */
  const openRun = $derived(store.current ? (store.pending[relPath(store, store.current)] ?? null) : null)
  /** Any run at all is in flight. What actually gates delegation — claude-agent
   *  serializes the whole `idea-proof` task, not one idea (see `runInFlight`). */
  const runBusy = $derived(runInFlight(store))
  /** The progress line to show next to the hourglass, or ''. */
  const openLast = $derived(
    store.current && runProgress?.ideaRel === relPath(store, store.current) ? runProgress.last : '',
  )

  /** Rotating grey-text prompt for a blank document. `newIdea()` advances
   *  `placeholderSeq` (and persists it), so a fresh draft gets the next of the
   *  five lines; both the kit and the fallback textarea show the same one. */
  const placeholder = $derived(pickPlaceholder(placeholderLines(), store.placeholderSeq))
  /** Narrowed once so the template can discriminate on `.kind`. */
  const saveState = $derived(store.saveState)

  /** Whatever the live editor holds — kit or fallback. */
  function markdown(): string {
    return kit ? kit.getMarkdown() : fallbackText
  }

  /**
   * Pushes markdown into the live editor and re-baselines the dirty check
   * against what the editor holds *afterwards* — see `showInEditor`: the kit
   * normalizes markdown on the way in and echoes the normalized form back
   * ~200 ms later, so baselining on the input would make an untouched document
   * report itself dirty.
   */
  function showMarkdown(md: string): void {
    if (!kit) fallbackText = md
    showInEditor(store, kit, md)
  }

  /**
   * Every write to an idea file goes through this one autosave instance —
   * nothing else calls `saveIdea`. That is what makes the writes serial: the
   * autosave keeps at most one save in flight and re-runs afterwards if the
   * document moved on, so a Cmd+S landing on top of a debounce timer can't
   * have an older buffer finish last and overwrite a newer one.
   *
   * `saveIdea` reports its own failures (`saveState` + a toast) and never
   * rejects, so there is nothing here to catch.
   */
  const autosave = createAutosave(async () => {
    if (cannotSave()) return
    await saveIdea(markdown())
  })

  /**
   * The kit's change callback. Only a document that actually differs from the
   * baseline schedules a write: `setMarkdown` dispatches a transaction and the
   * change plugin echoes it back ~200 ms later without distinguishing it from a
   * keystroke, so scheduling unconditionally would re-save every idea the
   * moment it was *opened* — restamping its frontmatter and touching files
   * nobody edited. `markEdited` has just computed exactly that comparison.
   */
  function onEdited(md: string): void {
    markEdited(store, md)
    if (store.dirty) autosave.schedule()
  }

  /**
   * Saving is pointless — and was once actively harmful — before the startup
   * sequence settles: `boot()` sets `vaultRoot` before it reads the state file
   * and lists the directory, and the kit isn't mounted yet, so `markdown()`
   * reports the empty fallback. (A blank document is refused by `saveIdea`
   * anyway; this keeps the intent explicit rather than resting on that.)
   *
   * `busy` is deliberately NOT part of this any more. It used to gate the save
   * button, which no longer exists; treating a concurrent settings write as
   * "cannot save" would now silently drop an autosave tick instead of merely
   * greying out a button.
   */
  function cannotSave(): boolean {
    return store.booting || store.needVault
  }

  /**
   * Writes the current buffer to disk *now* and resolves once it has landed
   * (`flush` awaits the in-flight save). Used wherever waiting 1.5s is not an
   * option: Cmd/Ctrl+S, switching ideas, starting a new one, switching modes.
   *
   * The `schedule()` first is not redundant: `flush()` alone only completes a
   * save that was already pending, and the kit's own 200 ms onChange debounce
   * means the last keystrokes may not have reached `onEdited` yet. Asking the
   * live buffer keeps an unchanged document from being rewritten on every
   * Cmd+S.
   */
  async function saveNow(): Promise<void> {
    if (needsSaveBefore(store, markdown())) autosave.schedule()
    await autosave.flush()
  }

  /**
   * Swapping the editor's content destroys whatever is in it, and a draft that
   * has never been saved has no undo path — so unsaved changes are written to
   * disk first, and a failed write aborts the switch rather than proceeding to
   * overwrite the buffer.
   *
   * The question is put to the **live buffer**, not to `store.dirty`: the flag
   * trails the editor by a 200 ms debounce, so a paragraph typed just before
   * the click would slip through an unset flag. An untouched document matches
   * its baseline byte for byte (see `rebaseline`), so merely browsing the
   * inbox still never writes anything.
   *
   * A blank draft is let through: by design it is never given a file, so there
   * is nothing to lose and nothing that could fail.
   */
  async function keepUnsaved(): Promise<boolean> {
    const md = markdown()
    if (!needsSaveBefore(store, md)) return true
    if (isBlank(md)) return true
    if (cannotSave()) {
      // Refusing silently would read as a dead click. Say why nothing happened.
      toast(t('unsavedWarning'))
      return false
    }
    await saveNow()
    // Assert the postcondition against the buffer itself rather than inferring
    // it from `saveState`. `saveState` is a report *about* a save, and reading
    // it here asks the wrong question twice over: it can still say `saving`
    // (another writer's save is in flight) and it says nothing at all about
    // whether the bytes on disk match what the editor holds *now* — a save
    // that succeeded on an older buffer would read as success. The buffer
    // comparison is the thing that actually has to be true before the content
    // may be thrown away, so ask that.
    if (!needsSaveBefore(store, markdown())) return true
    toast(t('unsavedWarning'))
    return false
  }

  async function pick(name: string): Promise<void> {
    if (!(await keepUnsaved())) return
    const body = await loadIdea(name)
    if (body !== null) showMarkdown(body)
  }

  async function startNew(): Promise<void> {
    if (!(await keepUnsaved())) return
    showMarkdown(newIdea())
    refreshPlaceholder()
    kit?.focus()
  }

  /**
   * Pushes the rotation's current line into the kit. `newIdea()` advances
   * `placeholderSeq`, but the kit read `opts.placeholder` once at mount and
   * would otherwise show the window's opening line forever — the rotation
   * would only ever be visible across window restarts, and "click New again
   * and you get a different prompt" is precisely what the design promises.
   *
   * Read from the store rather than from the `placeholder` `$derived` above so
   * the value handed over is unambiguously the post-increment one. The
   * degraded textarea needs nothing here: it binds that derived directly.
   */
  function refreshPlaceholder(): void {
    kit?.setPlaceholder(pickPlaceholder(placeholderLines(), store.placeholderSeq))
  }

  /**
   * Deleting and renaming both have to reach the disk BEFORE they run, or an
   * autosave still in flight lands afterwards — recreating the file that was
   * just deleted, or writing the editor's content back under the old name. The
   * `saveNow()` here is exactly that barrier: `flush()` resolves only once the
   * in-flight write has settled, so nothing can arrive behind the mutation.
   *
   * It is a barrier even when the row being acted on is NOT the open document:
   * what must not be in flight is *any* write, and the only writer is this
   * window's autosave.
   *
   * A failed flush does not abort either action. `saveIdea` has already
   * reported it (and, for a delete, the user's answer to "delete this idea?"
   * is not made less true by a failed write to it).
   */
  async function removeIdea(name: string): Promise<void> {
    await saveNow()
    const blank = await deleteIdea(name)
    // Non-null means the document that was deleted is the one on screen: the
    // store has already detached it, and the editor still holds its text, so
    // the blank draft has to be pushed in here.
    if (blank !== null) {
      showMarkdown(blank)
      // Same reason as `startNew`: the store detached the document through
      // `newIdea()`, which advanced the rotation.
      refreshPlaceholder()
      kit?.focus()
    }
  }

  async function rename(from: string, raw: string): Promise<boolean> {
    await saveNow()
    return await renameIdea(from, raw)
  }

  async function switchMode(m: KitMode): Promise<void> {
    if (!kit || m === mode) return
    await saveNow() // the panes hand the document over; land it on disk first
    await kit.setMode(m) // flushes any pending onChange before switching
    mode = kit.getMode()
    kit.focus()
  }

  // ── delegation ────────────────────────────────────────────────────────────
  //
  // The chain, end to end: flush the buffer to disk (claude-agent
  // `canonicalize`s the path it is given, so the file has to exist) → seed the
  // task template and call `host.agent.run` (`agent-client.ts`) → record the
  // run in `pending` AND write that to disk at once → poll for inline progress
  // until the run ends.
  //
  // What this window deliberately does NOT own: the notification. The run
  // outlives the window (closing it tears the plugin down, polling included),
  // so the tray reminder is claude-agent's job — it is handed the two titles
  // and the paths in the `notify` spec and pushes exactly one reminder itself.
  // Sending a second one from here would show the user two, since the tray
  // registry does not deduplicate.

  /** Consecutive failed status calls before a watcher gives up on a run. */
  const MAX_POLL_ERRORS = 5
  /** run_id → the timer for its next poll. NOT `$state`: nothing renders these
   *  handles, and making them reactive would invalidate the action bar twice a
   *  second for nothing. */
  const polls = new Map<string, ReturnType<typeof setTimeout>>()
  /** Set by `onMount`'s teardown; every scheduled callback checks it. */
  let unmounted = false

  /**
   * Follows one run to its end, purely so the window can say "⧖ arguing" and
   * show what the agent is doing. A `setTimeout` chain rather than an interval
   * (a slow status call can never stack up behind itself) and never an
   * `$effect` — an effect that synchronously calls store functions which read
   * and write `$state` self-invalidates into a loop that freezes the whole
   * window (MEMORY feedback_svelte_effect_untrack).
   *
   * Stopping is not a failure mode: the run belongs to claude-agent, so a
   * window that closes mid-run loses the progress line and nothing else — the
   * reminder still arrives, and `reconcilePending` picks the run up next time.
   */
  function watch(ideaRel: string, runId: string): void {
    if (unmounted || polls.has(runId)) return
    let errors = 0

    const schedule = () => {
      if (!unmounted) polls.set(runId, setTimeout(() => void poll(), POLL_MS))
    }

    const poll = async () => {
      polls.delete(runId)
      if (unmounted) return

      let view: RunView
      try {
        view = interpretStatus(await agentStatus(TASK_ID, runId))
        errors = 0
      } catch (e) {
        console.warn('[idea-spark] a run status poll failed:', e)
        // Keep asking — but not forever. An agent disabled mid-run would
        // otherwise reject every two seconds for as long as the window stays
        // open. Giving up leaves `pending` untouched on purpose: "I can't
        // reach the agent" is not evidence that the run failed, and startup
        // reconciliation will ask again.
        errors += 1
        if (errors < MAX_POLL_ERRORS) schedule()
        return
      }
      if (unmounted) return

      if (view.kind === 'running') {
        runProgress = { ideaRel, last: view.last }
        schedule()
        return
      }

      if (runProgress?.ideaRel === ideaRel) runProgress = null
      // `finishRun` drops the pending entry (memory + disk), re-lists the
      // directory and — on success — raises `celebrate`, which is what
      // `<Celebration/>` is watching. `null` means the run was already
      // settled by something else; say nothing rather than toasting twice.
      const outcome = await finishRun(runId, runStatusWord(view))
      if (outcome === 'failed') toast(t('delegateFailed'), 'error')
    }

    schedule()
  }

  /**
   * Hands one idea to the agent. `name` names the row the inbox's context menu
   * was opened on; omitted, it means the document in the editor.
   *
   * The `saveNow()` is load-bearing twice over: `note_path` must point at a
   * file that already exists, and — for a never-saved draft — the save is what
   * gives the idea a file name to delegate at all. It is also the same barrier
   * delete and rename use: no write may still be in flight behind the run.
   */
  async function delegate(name?: string): Promise<void> {
    const vaultRoot = store.vaultRoot
    if (delegating || store.needVault || !vaultRoot) return
    // One run at a time, across ALL ideas — see `runInFlight` for why that is
    // claude-agent's rule rather than ours. The button and the row menu are
    // both disabled on this same predicate; this is the backstop, and it says
    // why out loud instead of swallowing the click.
    if (runInFlight(store)) {
      toast(t('delegateBusy'), 'error')
      return
    }
    // Claimed BEFORE the flush, not after it: `saveNow()` is an await, and a
    // second click landing inside it would otherwise sail past this guard and
    // start a second run on the same idea — whose id would take the first
    // one's place in `pending`, leaving that run unwatched and its proof
    // document overwritten by whichever run finished last.
    delegating = true
    try {
      await saveNow()

      const target = name ?? store.current
      // The flush is the ONLY thing standing between the agent and a stale
      // file, and it cannot report its own failure: `saveNow` never rejects
      // and `saveIdea` only writes `saveState` — which the action bar is about
      // to cover with "⧖ arguing" for the whole run. So assert the
      // postcondition against the buffer itself, exactly as `keepUnsaved`
      // does: if what the editor holds did not reach the disk, the agent would
      // argue a version the user has already moved on from. Only checked when
      // the target IS the open document — another row's file is not what the
      // editor is holding.
      if ((target === null || target === store.current) && needsSaveBefore(store, markdown())) {
        toast(t('unsavedWarning'), 'error')
        return
      }
      // No file: the draft is blank, and by design a blank idea is never
      // written. (A *failed* save no longer lands here — the check above
      // catches it and says something true instead of "write something".)
      if (!target) {
        toast(t('delegateEmpty'), 'error')
        return
      }
      const ideaRel = relPath(store, target)

      const result = await delegateIdea(ideaRel, titleOf(store, target), vaultRoot)
      if (!result.ok) {
        if (result.reason === 'agent-missing') agentMissing = true
        else toast(result.message, 'error')
        return
      }
      // Registered in memory and on disk in the same breath. A run that exists
      // only in memory is lost to a window close, and nothing would ever
      // reconcile it: the idea would sit there as a draft while claude-agent
      // quietly finished arguing it.
      markPending(store, ideaRel, result.runId)
      await persist()
      watch(ideaRel, result.runId)
      toast(t('waitHint'))
    } finally {
      delegating = false
    }
  }

  /** Focuses the agent-missing layer so Esc reaches it (see its `onkeydown`). */
  function takeFocus(node: HTMLElement) {
    node.focus()
  }

  onMount(() => {
    let disposed = false

    // Host→UI pushes. `theme-changed` is the kit's business (it registers its
    // own listener; the bridge fans out to every subscriber). Nothing else is
    // expected: a run reports through `host.agent.status` (polled below), not
    // through a push — claude-agent posts its run events to its OWN window.
    // An unknown payload is therefore logged and ignored, not an error.
    bridge().onMessage((payload) => {
      if ((payload as { type?: string } | null)?.type === 'theme-changed') return
      console.debug('[idea-spark] unhandled host push:', payload)
    })

    void (async () => {
      await boot()
      if (disposed || store.needVault) return

      const initial = newIdea()
      fallbackText = initial
      await tick()
      if (disposed) return

      try {
        if (!editorEl) throw new Error('editor container missing')
        const mount = await loadKit()
        if (disposed) return
        kit = await mount(editorEl, {
          initialMarkdown: initial,
          mode,
          placeholder,
          baseDir: store.ideaDir,
          onChange: onEdited,
        })
        if (disposed) {
          kit.destroy()
          kit = null
          return
        }
        // The live buffer is already ≠ the template the moment the kit mounts:
        // the template ends with a trailing newline and ProseMirror's markdown
        // serializer drops it. (The mount itself emits nothing — `createEditor`
        // only builds an `EditorState`, it dispatches no transaction, so the
        // change plugin never echoes.) Without rebaselining off the kit, the
        // very first document a user ever sees is born dirty and the first
        // keystroke-free autosave writes a document nobody edited.
        rebaseline(store, kit)
        kit.focus()
      } catch (e) {
        console.error('[idea-spark] the editor kit failed to load:', e)
        // `rebaseline`/`focus` run *after* `kit` is assigned, so a throw there
        // would otherwise leave `kitFailed === true` with a live `kit`: the UI
        // renders the fallback <textarea> (bound to `fallbackText`) while
        // `markdown()` keeps reading a kit Svelte has already torn out of the
        // DOM — every later save would persist the wrong text. Drop the kit so
        // the fallback is the single source of truth.
        try { kit?.destroy() } catch { /* already broken — nothing left to salvage */ }
        kit = null
        store.kitFailed = true
      }

      // Runs recorded by an EARLIER window: ask once where each one stands
      // (done → folded in and dropped; lost → marked failed; still running →
      // watched again for its progress line). Last, and unawaited by the mount
      // path above it, because it may have to wake claude-agent up — the
      // editor must not wait on that.
      if (disposed) return
      for (const { ideaRel, runId } of await reconcilePending()) {
        if (disposed) return
        watch(ideaRel, runId)
      }
    })()

    // Capture phase: the editor panes handle their own keys, and Cmd/Ctrl+S
    // must win regardless of where focus sits. With autosave running, this is
    // "write it right now" rather than the only way to save.
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void saveNow()
      }
    }
    window.addEventListener('keydown', onKeyDown, true)

    // Best effort (per spec): a closing webview may never run this, and it
    // cannot be awaited — the flush is fired and the warning stays as the
    // user-visible fallback for the case where the write doesn't make it.
    const onBeforeUnload = () => {
      // Live buffer again, not `dirty` — the last keystrokes before a close are
      // exactly the ones still inside the debounce window.
      if (!needsSaveBefore(store, markdown())) return
      void saveNow()
      // Warn for a blanked document too: `saveIdea` refuses to write it (see
      // `isBlank`), so "you deleted everything and closed the window" is
      // precisely a case where what is on screen never reached the disk.
      toast(t('unsavedWarning'))
    }
    window.addEventListener('beforeunload', onBeforeUnload)

    return () => {
      disposed = true
      // Stops every status poll: the flag blocks the ones already in flight
      // from rescheduling, and the loop drops the timers that are waiting.
      // The RUNS are untouched by this — they belong to claude-agent, which
      // still delivers their tray reminders, and `reconcilePending` picks
      // them up when this window next opens.
      unmounted = true
      for (const id of polls.values()) clearTimeout(id)
      polls.clear()
      window.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('beforeunload', onBeforeUnload)
      autosave.dispose() // drops the pending timer; the window is going away
      kit?.destroy() // flushes a pending onChange first
      kit = null
    }
  })
</script>

<main class="app">
  {#if store.needVault}
    <div class="notice">{t('needVault')}</div>
  {:else}
    <div class="content">
      <section class="editor-col">
        {#if store.kitFailed}
          <p class="warn">{t('editorUnavailable')}</p>
          <textarea
            class="fallback"
            bind:value={fallbackText}
            {placeholder}
            spellcheck="false"
            oninput={() => onEdited(fallbackText)}
          ></textarea>
        {:else}
          <div class="editor" bind:this={editorEl}></div>
          <!-- Floating over the editor's top-right corner. Not rendered in the
               degraded textarea case: there are no modes to switch between. -->
          <div class="float-toggle">
            <ModeToggle {mode} onchange={switchMode} />
          </div>
        {/if}
      </section>

      <!-- Hidden by default (design §1); the action bar's tray button toggles
           it and the choice is remembered across windows (`toggleInbox` →
           `inboxOpen`). -->
      {#if store.inboxOpen}
        <InboxPanel
          onselect={pick}
          ondelete={removeIdea}
          onrename={rename}
          ondelegate={(name) => void delegate(name)}
        />
      {/if}
    </div>

    <div class="actionbar">
      <!-- The run on the OPEN document outranks the save state: while an idea
           is being argued, "⧖ arguing" is the thing the bar is for. Every
           other idea's run shows as the same hourglass on its own inbox row,
           at 12px (`statusOf` → `STATUS_MARK`). -->
      <span class="savestate">
        {#if delegating}
          {t('delegating')}
        {:else if openRun}
          <span class="running">
            <Icon name="running" />
            <!-- The text is its own element so it — and not the icon — is what
                 gets clipped when a long "Read some-very-long-name.md"
                 progress line runs out of bar. -->
            <span class="runtext">{t('statusRunning')}{openLast ? ` · ${openLast}` : ''}</span>
          </span>
        {:else if saveState.kind === 'saving'}
          {t('saving')}
        {:else if saveState.kind === 'saved'}
          {t('saved')} {saveState.at}
        {:else if saveState.kind === 'failed'}
          <button type="button" class="failed" title={saveState.message} onclick={saveNow}>
            {t('saveFailed')} · {t('retry')}
          </button>
        {/if}
      </span>
      <div class="spacer"></div>
      <!-- Icon *and* label on these two: they are the window's two verbs, and
           the icon is decorative (`aria-hidden` inside `Icon`) — the button's
           accessible name is still the text next to it. -->
      <button type="button" class="ghost" onclick={startNew}>
        <Icon name="new-idea" />
        {t('newIdea')}
      </button>
      <button
        type="button"
        class="ghost"
        disabled={delegating || runBusy}
        title={runBusy ? t('delegateBusy') : t('delegate')}
        onclick={() => void delegate()}
      >
        <Icon name="delegate" />
        {delegating ? t('delegating') : t('delegate')}
      </button>
      <!-- The two icon-only buttons carry their whole meaning in `aria-label`
           and `title`; that was true when they were emoji and it is what must
           NOT be dropped now that the glyph is an `aria-hidden` SVG. -->
      <button
        type="button"
        class="icon"
        aria-pressed={store.inboxOpen}
        aria-label={t('inbox')}
        title={t('inbox')}
        onclick={toggleInbox}
      >
        <Icon name="inbox" />
      </button>
      <button
        type="button"
        class="icon"
        aria-label={t('settings')}
        aria-expanded={settingsOpen}
        title={t('settings')}
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        <Icon name="settings" />
      </button>
      {#if settingsOpen}
        <!-- Changing the idea directory detaches the open document, so the
             buffer has to reach the disk in the OLD directory first — otherwise
             the next autosave tick deposits it as a new file in the new one and
             the idea exists twice.

             `keepUnsaved` rather than `saveNow`, for the same reason `pick` and
             `startNew` use it: it flushes AND asserts that the flush landed
             (`saveNow` never rejects, so a failed write would sail through),
             answering false when the buffer is still ahead of the disk. The
             question it asks — "may this document be let go of?" — is exactly
             the question a directory change poses. -->
        <SettingsPopover onclose={() => (settingsOpen = false)} onbeforecommit={keepUnsaved} />
      {/if}
    </div>
  {/if}

  <!-- claude-agent isn't installed (or has been disabled): the one thing the
       user can do about it is install it, so say that rather than showing a
       raw `agent_unavailable:` error in a toast that scrolls away. -->
  {#if agentMissing}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (agentMissing = false)}></div>
    <div
      class="layer"
      role="dialog"
      aria-modal="true"
      aria-labelledby="agent-missing-title"
      tabindex="-1"
      use:takeFocus
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          agentMissing = false
        }
      }}
    >
      <h2 id="agent-missing-title">{t('agentMissing')}</h2>
      <p>{t('agentMissingHint')}</p>
      <div class="layer-actions">
        <button type="button" class="ghost" onclick={() => (agentMissing = false)}>{t('close')}</button>
      </div>
    </div>
  {/if}

  <Celebration />
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
  }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
  }
  .spacer { flex: 1; }
  .notice {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.65;
    font-size: 0.9rem;
  }
  .content {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  /* `position: relative` anchors the floating ModeToggle; the determinate
     height chain (flex:1 + min-height:0) is what keeps the kit's source mode
     from collapsing to zero height. */
  .editor-col {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .float-toggle {
    position: absolute;
    top: 0;
    right: 12px;
    z-index: 10;
  }
  /* The kit sizes itself with height:100% + absolute positioning, so its
     container MUST have a determinate height — a bare flex child would let
     source mode collapse to zero. */
  .editor {
    flex: 1;
    /* `display:flex` (not just `flex:1`) so `.kit-host`'s `height:100%` has a
       stretched flex item to resolve against no matter how the browser treats
       percentage heights inside a flex item. */
    display: flex;
    position: relative;
    min-height: 0;
    overflow: hidden;
  }
  .warn {
    flex: 0 0 auto;
    margin: 0;
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    background: color-mix(in srgb, #dc2626 12%, transparent);
  }
  .fallback {
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
    padding: 0.75rem;
    border: 0;
    resize: none;
    background: Canvas;
    color: CanvasText;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9rem;
    line-height: 1.55;
  }
  .fallback:focus { outline: none; }
  /* 38px, fixed: the whole point of the layout is that everything above it is
     writing surface. `position: relative` anchors SettingsPopover, which opens
     upward from here. */
  .actionbar {
    position: relative;
    flex: 0 0 38px;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0 0.75rem;
    border-top: 1px solid var(--line, #e5e7eb);
  }
  .savestate {
    font-size: 0.78rem;
    opacity: 0.6;
    white-space: nowrap;
  }
  .savestate .failed {
    padding: 0;
    border: 0;
    background: none;
    color: #dc2626;
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .savestate:has(.failed) { opacity: 1; }
  /* Icon + text on one line. `min-width: 0` on both is what lets the ellipsis
     below actually engage: a flex item defaults to `min-width: auto` and would
     refuse to shrink below its text, pushing the buttons off the bar instead. */
  .savestate .running {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 100%;
    min-width: 0;
    vertical-align: middle;
  }
  .savestate .runtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The run indicator is the one thing in this slot that is about work in
     flight rather than about a save that already happened — hence the fuller
     opacity. The truncation of a long "Read some-very-long-name.md" is done by
     `.runtext` above, not here: this slot's only child is then an atomic
     inline-flex box, which `text-overflow` cannot act on. `min-width: 0` +
     `overflow: hidden` stay as the backstop that keeps whatever `.runtext`
     hasn't already clipped from pushing the buttons off the bar. */
  .savestate:has(.running) {
    opacity: 0.85;
    min-width: 0;
    overflow: hidden;
  }
  /* Flex so the 16px icon and the label sit on a shared centre line — an
     inline SVG would otherwise hang off the text baseline and make the bar
     look crooked. */
  .actionbar > button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--line, #d1d5db);
    background: none;
    color: inherit;
  }
  .actionbar > button:disabled { opacity: 0.5; cursor: default; }
  /* Scoped through `.actionbar >` so these beat the generic button rule above
     on specificity (0-2-1 vs 0-1-1) rather than on source order. */
  .actionbar > button.icon {
    /* Square padding around a square 16px glyph: the old value was tuned for
       an emoji's own side bearings, which an SVG does not have. */
    padding: 0.3rem;
    border: 0;
    background: none;
    color: inherit;
  }
  .actionbar > button.icon:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  /* The agent-missing layer. Same geometry as ConfirmDialog's, minus the file
     list and the destructive button — there is nothing to confirm here. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.28);
  }
  .layer {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(420px, calc(100vw - 2rem));
    box-sizing: border-box;
    padding: 1rem;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 10px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.28);
  }
  .layer:focus { outline: none; }
  .layer h2 {
    margin: 0 0 0.4rem;
    font-size: 0.95rem;
  }
  .layer p {
    margin: 0;
    font-size: 0.82rem;
    line-height: 1.45;
    opacity: 0.8;
  }
  .layer-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.9rem;
  }
  .layer-actions button {
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
</style>
