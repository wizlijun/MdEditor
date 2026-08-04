<!-- App.svelte — the Idea Spark window: a blank page you write on.

     Layout (design §1: "open it and write"). No title bar at all — the window
     chrome is Tauri's job — so the editor starts at the very top edge and runs
     down to a single 38px action bar:

       ┌───────────────────────┬─────────┐
       │              ┌──────┐ │ inbox   │  ← ModeToggle, floating (absolute)
       │ editor       │👁│</>│ │ 240px,  │
       │ (flex:1)     └──────┘ │ optional│  ← InboxPanel; 📥 toggles it and the
       ├───────────────────────┴─────────┤     choice is persisted (`inboxOpen`)
       │ saved 19:42   New  Delegate 📥 ⚙│  ← 38px action bar
       └─────────────────────────────────┘

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
  import InboxPanel from './components/InboxPanel.svelte'
  import ModeToggle from './components/ModeToggle.svelte'
  import SettingsPopover from './components/SettingsPopover.svelte'
  import { createAutosave } from './lib/autosave'
  import { bridge } from './lib/bridge'
  import { loadKit, type KitEditor, type KitMode } from './lib/editor-kit'
  import { pickPlaceholder, placeholderLines } from './lib/placeholder'
  import {
    boot,
    deleteIdea,
    isBlank,
    loadIdea,
    markEdited,
    needsSaveBefore,
    newIdea,
    rebaseline,
    renameIdea,
    saveIdea,
    showInEditor,
    state as store,
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
    kit?.focus()
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

  onMount(() => {
    let disposed = false

    // Host→UI pushes. `theme-changed` is the kit's business (it registers its
    // own listener; the bridge fans out to every subscriber), and run
    // completions only start arriving once delegation is wired (Task 13).
    // Until then an unknown payload is deliberately ignored, not an error.
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

      <!-- Hidden by default (design §1); the action bar's 📥 toggles it and the
           choice is remembered across windows (`toggleInbox` → `inboxOpen`). -->
      {#if store.inboxOpen}
        <InboxPanel onselect={pick} ondelete={removeIdea} onrename={rename} />
      {/if}
    </div>

    <div class="actionbar">
      <span class="savestate">
        {#if saveState.kind === 'saving'}
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
      <button type="button" class="ghost" onclick={startNew}>{t('newIdea')}</button>
      <button
        type="button"
        class="ghost"
        disabled
        title={t('delegateDeferred')}
        aria-describedby="delegate-hint">{t('delegate')}</button
      >
      <span id="delegate-hint" class="sr-only">{t('delegateDeferred')}</span>
      <button
        type="button"
        class="icon"
        aria-pressed={store.inboxOpen}
        aria-label={t('inbox')}
        title={t('inbox')}
        onclick={toggleInbox}>📥</button
      >
      <button
        type="button"
        class="icon"
        aria-label={t('settings')}
        aria-expanded={settingsOpen}
        title={t('settings')}
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        ⚙
      </button>
      {#if settingsOpen}
        <SettingsPopover onclose={() => (settingsOpen = false)} />
      {/if}
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
  .actionbar > button {
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
    padding: 0.15rem 0.4rem;
    border: 0;
    background: none;
    color: inherit;
    font-size: 1rem;
  }
  .actionbar > button.icon:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
