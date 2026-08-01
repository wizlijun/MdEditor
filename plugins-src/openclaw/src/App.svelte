<!-- src/App.svelte — openclaw v2 chat window root (ported from v1 chat-app.svelte). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { t } from './lib/strings'
  import { describeError } from './lib/errors'
  import { start, stop } from './lib/openclaw/client.svelte'
  import SessionPicker from './components/chat/SessionPicker.svelte'
  import MessageList from './components/chat/MessageList.svelte'
  import Composer from './components/chat/Composer.svelte'
  import PendingClaimToast from './components/chat/PendingClaimToast.svelte'
  import RemoteOnboarding from './components/chat/RemoteOnboarding.svelte'

  let mode = $state<'detecting' | 'host' | 'remote' | 'needs-pairing'>('detecting')
  let initError = $state<string | null>(null)

  async function init() {
    initError = null
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

  onMount(() => { init(); return () => stop() })
</script>

{#if mode === 'detecting'}
  <p>{t('chat.detecting')}</p>
{:else if mode === 'needs-pairing'}
  <RemoteOnboarding onComplete={() => init()} />
{:else}
  {#if initError}
    <div class="init-error">⚠️ {t('chat.initError')}: {initError}</div>
  {/if}
  <main>
    <SessionPicker />
    <MessageList />
    <Composer />
  </main>
  <PendingClaimToast />
{/if}

<style>
  :global(:root) { color-scheme: light dark; }
  :global(body) { margin: 0; font-family: -apple-system, system-ui, sans-serif; }
  main { display: flex; flex-direction: column; height: 100vh; }
  .init-error {
    background: #fef3c7;
    color: #92400e;
    padding: 0.5rem 0.75rem;
    font-size: 0.8rem;
    border-bottom: 1px solid #e5e7eb;
  }
</style>
