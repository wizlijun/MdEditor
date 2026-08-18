<!-- App.svelte — the Trace Source window: a one-shot delegation composer.

     Same visual grammar as idea-spark (this window IS its trimmed sibling):
     no title bar content of ours — the editor starts at the top edge and runs
     down to a single 38px action bar.

       ┌─────────────────────────────────┐
       │ editor (flex:1)                 │  ← quoted passage + scope notes
       ├─────────────────────────────────┤
       │ status        New  Trace  by ▾  │  ← 38px bar
       └─────────────────────────────────┘

     What it deliberately does NOT have: files, autosave, an inbox, modes, a
     settings panel. The composed text lives exactly as long as the delegation
     it is for; the durable artifact is the report the agent writes under
     traces/, and the notification for that comes from the agent plugin — this
     window may well be closed by then.

     The passage arrives prefilled from the host's right-click 「溯源」 item —
     via the window URL's `?seed=` on a fresh window, via a `seed` host push on
     one that is already open. A new seed REPLACES the buffer: reaching for
     the tool again means "trace this now", and the previous composition was
     either already delegated or abandoned.

     Declares `color-scheme: light dark` so this standalone window follows the
     system appearance (MEMORY reference_webview_color_scheme). Startup runs in
     `onMount`, never in an `$effect` (MEMORY feedback_svelte_effect_untrack). -->
<script lang="ts">
  import { onMount, tick } from 'svelte'
  import Icon from './components/Icon.svelte'
  import AgentPicker from './lib/agent-picker/AgentPicker.svelte'
  import {
    rememberProvider,
    rememberedProvider,
    type AgentOption,
  } from './lib/agent-picker/types'
  import { agentProviders, bridge, vaultInfo } from './lib/bridge'
  import { delegateTrace } from './lib/delegate'
  import { loadKit, type KitEditor } from './lib/editor-kit'
  import { setLocale, t, tv } from './lib/strings'

  setLocale(bridge().locale)

  let editorEl: HTMLDivElement | undefined = $state()
  let kit: KitEditor | null = null
  /** Editor content while the kit is unavailable (the degraded textarea). */
  let fallbackText = $state('')
  let kitFailed = $state(false)
  let booting = $state(true)
  let needVault = $state(false)
  let vaultRoot = $state<string | null>(null)

  /** A `host.agent.run` call is in flight (between the click and the run id). */
  let delegating = $state(false)
  /** The last delegation went out; cleared by the next edit or New. */
  let delegated = $state(false)
  /** A failed delegation's message — shown in the bar, not in a toast that
   *  scrolls away (this window has nothing else competing for that slot). */
  let errorMsg = $state('')
  /** No agent plugin installed — the layer pointing at the market. */
  let agentMissing = $state(false)

  // ── which agent runs this trace ───────────────────────────────────────────
  // Remembered per surface, same convention as every other delegation surface.
  const AGENT_SURFACE = 'trace-source'
  let agents: AgentOption[] = $state([])
  let agentId: string | undefined = $state(undefined)

  async function loadAgents() {
    try {
      const r = await agentProviders()
      agents = r.providers ?? []
      agentId = rememberedProvider(
        AGENT_SURFACE,
        agents.map((a) => a.id),
        r.default,
      )
    } catch {
      // An older host without host.agent.providers, or no agent installed. The
      // picker hides itself and the run goes to whatever the host picks.
      agents = []
      agentId = undefined
    }
  }

  function pickAgent(id: string) {
    agentId = id
    rememberProvider(AGENT_SURFACE, id)
  }

  /** Whatever the live editor holds — kit or fallback. */
  function markdown(): string {
    return kit ? kit.getMarkdown() : fallbackText
  }

  function showMarkdown(md: string): void {
    if (kit) kit.setMarkdown(md)
    else fallbackText = md
  }

  /** A keystroke after a delegation starts the next composition: the "it went
   *  out" status would otherwise claim the NEW text was already sent. */
  function onEdited(md: string): void {
    fallbackText = md
    if (delegated) delegated = false
    if (errorMsg) errorMsg = ''
  }

  function applySeed(text: string): void {
    showMarkdown(text)
    delegated = false
    errorMsg = ''
    kit?.focus()
  }

  function startNew(): void {
    applySeed('')
  }

  async function delegate(): Promise<void> {
    if (delegating || booting || needVault || !vaultRoot) return
    const text = markdown()
    if (!text.trim()) {
      errorMsg = t('delegateEmpty')
      return
    }
    delegating = true
    try {
      const r = await delegateTrace(text, vaultRoot, agentId)
      if (!r.ok) {
        if (r.reason === 'agent-missing') agentMissing = true
        else errorMsg = r.message
        return
      }
      delegated = true
      errorMsg = ''
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
    // own listener); `seed` replaces the buffer (see the header comment);
    // `tray-activate` has no tray behind it here — logged and ignored.
    bridge().onMessage((payload) => {
      const type = (payload as { type?: string } | null)?.type
      if (type === 'theme-changed') return
      if (type === 'seed') {
        const text = (payload as { payload?: { text?: string } } | null)?.payload?.text
        if (typeof text === 'string' && text) applySeed(text)
        return
      }
      console.debug('[trace-source] unhandled host push:', payload)
    })

    void (async () => {
      try {
        const info = await vaultInfo()
        vaultRoot = info.root
        needVault = !info.root
      } catch (e) {
        console.error('[trace-source] host.vault.info failed:', e)
        needVault = true
      }
      booting = false
      if (disposed || needVault) return
      void loadAgents()

      // A fresh window gets its prefill through the URL query (an already-open
      // one gets a `seed` push instead). Read BEFORE the kit mounts so the
      // text can simply be the initial markdown.
      let initial = ''
      try {
        const raw = new URLSearchParams(location.search).get('seed')
        if (raw) {
          const seed = JSON.parse(raw) as { text?: string }
          if (typeof seed.text === 'string') initial = seed.text
        }
      } catch (e) {
        console.warn('[trace-source] bad seed query:', e)
      }
      fallbackText = initial

      await tick()
      if (disposed) return
      try {
        if (!editorEl) throw new Error('editor container missing')
        const mount = await loadKit()
        if (disposed) return
        kit = await mount(editorEl, {
          initialMarkdown: initial,
          mode: 'rich',
          placeholder: t('placeholder'),
          onChange: onEdited,
        })
        if (disposed) {
          kit.destroy()
          kit = null
          return
        }
        kit.focus()
      } catch (e) {
        console.error('[trace-source] the editor kit failed to load:', e)
        // Degrade to the textarea rather than leaving the window blank; drop
        // the kit so the fallback is the single source of truth.
        try { kit?.destroy() } catch { /* already broken */ }
        kit = null
        kitFailed = true
      }
    })()

    return () => {
      disposed = true
      kit?.destroy()
      kit = null
    }
  })
</script>

<main class="app">
  {#if needVault}
    <div class="notice">{t('needVault')}</div>
  {:else}
    <section class="editor-col">
      {#if kitFailed}
        <p class="warn">{t('editorUnavailable')}</p>
        <textarea
          class="fallback"
          bind:value={fallbackText}
          placeholder={t('placeholder')}
          spellcheck="false"
          oninput={() => onEdited(fallbackText)}
        ></textarea>
      {:else}
        <div class="editor" bind:this={editorEl}></div>
      {/if}
    </section>

    <div class="actionbar">
      <span class="status" class:error={!!errorMsg}>
        {#if delegating}
          {t('delegating')}
        {:else if errorMsg}
          {errorMsg}
        {:else if delegated}
          {t('delegated')}
        {/if}
      </span>
      <div class="spacer"></div>
      <button type="button" class="ghost" onclick={startNew}>
        <Icon name="trace" />
        {t('newTrace')}
      </button>
      <button
        type="button"
        class="ghost"
        disabled={delegating}
        onclick={() => void delegate()}
      >
        <Icon name="delegate" />
        {delegating ? t('delegating') : t('delegate')}
      </button>
      {#if agents.length}
        <AgentPicker
          options={agents}
          selected={agentId ?? null}
          disabled={delegating}
          onselect={pickAgent}
          label={tv as (k: string, v?: Record<string, string | number>) => string}
        />
      {/if}
    </div>
  {/if}

  <!-- No agent plugin: the one thing the user can do about it is install one,
       so say that instead of a raw `agent_unavailable:` message. -->
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
  .editor-col {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  /* The kit sizes itself with height:100% + absolute positioning, so its
     container MUST have a determinate height (see idea-spark's App.svelte). */
  .editor {
    flex: 1;
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
    position: relative;
    flex: 0 0 38px;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0 0.75rem;
    border-top: 1px solid var(--line, #e5e7eb);
  }
  .status {
    font-size: 0.78rem;
    opacity: 0.6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .status.error {
    color: #dc2626;
    opacity: 1;
  }
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
  /* The agent-missing layer — same geometry as idea-spark's. */
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
