<!-- App.svelte — the Idea Spark window: capture editor + action bar + history.
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
  import HistoryList from './components/HistoryList.svelte'
  import SettingsPopover from './components/SettingsPopover.svelte'
  import { bridge } from './lib/bridge'
  import { loadKit, type KitEditor, type KitMode } from './lib/editor-kit'
  import {
    boot,
    loadIdea,
    markEdited,
    needsSaveBefore,
    newIdea,
    rebaseline,
    saveIdea,
    showInEditor,
    state as store,
    toast,
  } from './lib/store.svelte'
  import { setLocale, t } from './lib/strings'

  setLocale(bridge().locale)

  let editorEl: HTMLDivElement | undefined = $state()
  let kit: KitEditor | null = null
  /** Editor content while the kit is unavailable (the degraded textarea). */
  let fallbackText = $state('')
  let mode = $state<KitMode>('rich')
  let settingsOpen = $state(false)

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

  function onEdited(md: string): void {
    markEdited(store, md)
  }

  /**
   * The save button's disabled condition, as a guard. `booting` matters as much
   * as `busy`: `boot()` sets `vaultRoot` before it reads the state file and
   * lists the directory, so a Cmd+S landing in that window would save an empty
   * document (the kit isn't mounted, `markdown()` returns ''), pin `current` to
   * that empty file for the rest of the session, and do it against a listing
   * that hasn't loaded yet.
   */
  function cannotSave(): boolean {
    return store.busy || store.booting || store.needVault
  }

  async function save(): Promise<void> {
    if (cannotSave()) return
    await saveIdea(markdown())
  }

  /**
   * Swapping the editor's content destroys whatever is in it, and a draft that
   * has never been saved has no undo path — the history list is a 240px rail
   * right next to the editor, so a mis-click must not cost the user their text.
   * Unsaved changes are therefore written to disk first (the user sees the
   * `saved` toast), and a failed save aborts the switch rather than proceeding
   * to overwrite the buffer.
   *
   * The question is put to the **live buffer**, not to `store.dirty`: the flag
   * trails the editor by a 200 ms debounce, so a paragraph typed just before
   * the click would slip through an unset flag. An untouched document matches
   * its baseline byte for byte (see `rebaseline`), so merely browsing the
   * history still never writes anything.
   */
  async function keepUnsaved(): Promise<boolean> {
    const md = markdown()
    if (!needsSaveBefore(store, md)) return true
    if (cannotSave()) {
      // Refusing silently would read as a dead click. Say why nothing happened.
      toast(t('unsavedWarning'))
      return false
    }
    return (await saveIdea(md)) !== null
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

  async function switchMode(m: KitMode): Promise<void> {
    if (!kit || m === mode) return
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
          placeholder: t('editorPlaceholder'),
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
    // must win regardless of where focus sits.
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void save()
      }
    }
    window.addEventListener('keydown', onKeyDown, true)

    // Best effort (per spec): a closing webview may never run this. It is a
    // reminder, not a guard — nothing blocks the close.
    const onBeforeUnload = () => {
      // Live buffer again, not `dirty` — the last keystrokes before a close are
      // exactly the ones still inside the debounce window.
      if (needsSaveBefore(store, markdown())) toast(t('unsavedWarning'))
    }
    window.addEventListener('beforeunload', onBeforeUnload)

    return () => {
      disposed = true
      window.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('beforeunload', onBeforeUnload)
      kit?.destroy() // flushes a pending onChange first
      kit = null
    }
  })
</script>

<main class="app">
  <header class="topbar">
    <h1>{t('title')}</h1>
    <div class="spacer"></div>
    <button
      class="icon"
      type="button"
      aria-label={t('settings')}
      title={t('settings')}
      onclick={() => (settingsOpen = !settingsOpen)}
    >
      ⚙
    </button>
    {#if settingsOpen}
      <SettingsPopover onclose={() => (settingsOpen = false)} />
    {/if}
  </header>

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
            placeholder={t('editorPlaceholder')}
            spellcheck="false"
            oninput={() => onEdited(fallbackText)}
          ></textarea>
        {:else}
          <div class="editor" bind:this={editorEl}></div>
        {/if}

        <div class="actionbar">
          {#if !store.kitFailed}
            <div class="modes">
              <button
                type="button"
                class:on={mode === 'rich'}
                title={t('modeRich')}
                aria-label={t('modeRich')}
                aria-pressed={mode === 'rich'}
                onclick={() => switchMode('rich')}>¶</button
              >
              <button
                type="button"
                class:on={mode === 'source'}
                title={t('modeSource')}
                aria-label={t('modeSource')}
                aria-pressed={mode === 'source'}
                onclick={() => switchMode('source')}>{'</>'}</button
              >
            </div>
          {/if}
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
          <!-- Same predicate as the Cmd/Ctrl+S path, so the shortcut can never
               do something the disabled button wouldn't. -->
          <button type="button" class="primary" disabled={cannotSave()} onclick={save}>
            {t('save')}
          </button>
        </div>
      </section>

      <HistoryList onselect={pick} />
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
  .topbar {
    position: relative;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--line, #e5e7eb);
  }
  .topbar h1 { margin: 0; font-size: 1rem; }
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
  .editor-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
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
  .actionbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 0.75rem;
    border-top: 1px solid var(--line, #e5e7eb);
  }
  .modes { display: flex; gap: 2px; }
  .modes button {
    width: 2rem;
    padding: 0.25rem 0;
    border: 1px solid var(--line, #d1d5db);
    border-radius: 6px;
    background: none;
    color: inherit;
    /* button inherits neither font-size nor family (MEMORY note). */
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .modes button.on { background: color-mix(in srgb, var(--accent, #2563eb) 20%, transparent); }
  .actionbar > button {
    padding: 0.3rem 0.8rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--line, #d1d5db);
    background: none;
    color: inherit;
  }
  .primary {
    border: 1px solid transparent;
    background: var(--accent, #2563eb);
    color: #fff;
  }
  .actionbar > button:disabled { opacity: 0.5; cursor: default; }
  .icon {
    padding: 0.15rem 0.4rem;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 1rem;
    cursor: pointer;
  }
  .icon:hover { background: color-mix(in srgb, currentColor 10%, transparent); }
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
