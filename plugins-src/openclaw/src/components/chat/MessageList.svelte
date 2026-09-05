<!-- src/components/chat/MessageList.svelte -->
<script lang="ts">
  import { t } from '../../lib/strings'
  import { state } from '../../lib/openclaw/client.svelte'
  import MessageBubble from './MessageBubble.svelte'

  const messages = $derived(state.currentSessionId ? (state.messagesBySession[state.currentSessionId] ?? []) : [])
</script>

<section class="list" aria-label={t('chat.messages')}>
  {#each messages as m (m.id)}
    <MessageBubble message={m} />
  {/each}
  {#if messages.length === 0}
    <p class="empty">{t('chat.noMessages')}</p>
  {/if}
</section>

<style>
  .list { display: flex; flex-direction: column; padding: 12px; overflow-y: auto; overflow-x: hidden; flex: 1; min-height: 0; min-width: 0; }
  .empty { color: var(--ui-secondary); text-align: center; margin-top: 2rem; }
</style>
