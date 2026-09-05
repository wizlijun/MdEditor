<!-- src/App.svelte — openclaw v2 chat window root (ported from v1 chat-app.svelte). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import '../../../src/styles/ui-foundation.css'
  import { t } from './lib/strings'
  import { describeError } from './lib/errors'
  import { start, stop, state as clientState } from './lib/openclaw/client.svelte'
  import SessionPicker from './components/chat/SessionPicker.svelte'
  import MessageList from './components/chat/MessageList.svelte'
  import Composer from './components/chat/Composer.svelte'
  import PendingClaimToast from './components/chat/PendingClaimToast.svelte'
  import RemoteOnboarding from './components/chat/RemoteOnboarding.svelte'

  let mode = $state<'detecting' | 'host' | 'remote' | 'needs-pairing'>('detecting')
  let initError = $state<string | null>(null)
  let reconnecting = $state(false)
  let messageDraft = $state('')

  async function init() {
    initError = null
    clientState.error = null
    try {
      const m = await start()
      mode = m === 'host' ? 'host' : 'remote'
    } catch (e) {
      const msg = String(e)
      console.error('[openclaw] connect failed:', msg)
      if (msg.includes('not paired')) {
        mode = 'needs-pairing'
      } else {
        initError = describeError(msg)
        mode = 'remote'
      }
    }
  }

  async function retry() {
    if (reconnecting) return
    reconnecting = true
    if (mode === 'needs-pairing') mode = 'detecting'
    try {
      await stop().catch(() => {})
      await init()
    } finally { reconnecting = false }
  }

  onMount(() => { void init(); return () => { void stop() } })
</script>

<main class="ui-surface">
  {#if mode === 'detecting'}
    <p class="loading" role="status">{t('chat.detecting')}</p>
  {:else if mode === 'needs-pairing'}
    <div class="onboarding"><RemoteOnboarding onComplete={retry} /></div>
  {:else}
    {#if initError || clientState.error || reconnecting}
      <div class="init-error" role="alert">
        <span>{reconnecting ? t('chat.connecting') : t('chat.initError') + ': ' + (initError ?? describeError(clientState.error ?? ''))}</span>
        <button type="button" disabled={reconnecting} onclick={retry}>{t('chat.retry')}</button>
      </div>
    {/if}
    <SessionPicker />
    <MessageList />
    <Composer bind:text={messageDraft} />
    <PendingClaimToast />
  {/if}
</main>

<style>
  :global(:root) { color-scheme: light dark; }
  :global(body) { margin: 0; font-family: -apple-system, system-ui, sans-serif; background: var(--ui-bg); color: CanvasText; }
  main { display: flex; flex-direction: column; height: 100vh; height: 100dvh; min-width: 0; overflow: hidden; background: var(--ui-bg); }
  .loading { margin: 24px; color: var(--ui-secondary); }
  .onboarding { flex: 1; min-height: 0; overflow: auto; }
  .init-error { flex: none; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; background: color-mix(in srgb, var(--ui-warning) 10%, var(--ui-surface)); color: var(--ui-warning); padding: 10px 12px; font-size: 13px; border-bottom: 1px solid var(--ui-separator); overflow-wrap: anywhere; }
  .init-error span { flex: 1 1 200px; min-width: 0; }
  .init-error button { border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: CanvasText; padding: 6px 10px; cursor: pointer; }
</style>
