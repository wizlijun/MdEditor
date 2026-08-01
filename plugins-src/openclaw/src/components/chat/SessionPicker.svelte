<!-- src/components/chat/SessionPicker.svelte -->
<script lang="ts">
  import { t, type MessageKey } from '../../lib/strings'
  import { state, newSession, openSession } from '../../lib/openclaw/client.svelte'
</script>

<header class="picker">
  <select
    value={state.currentSessionId ?? ''}
    onchange={(e) => openSession((e.target as HTMLSelectElement).value)}
  >
    {#each state.sessions as s (s.id)}
      <option value={s.id}>{s.title ?? s.id}</option>
    {/each}
  </select>
  <button onclick={() => newSession()}>{t('chat.newSession')}</button>
  <span class="status" data-status={state.status}>{t(('chat.status.' + state.status) as MessageKey)}</span>
</header>

<style>
  .picker { display: flex; align-items: center; gap: 0.5rem; padding: 0.5rem; border-bottom: 1px solid #e5e7eb; }
  select { flex: 1; padding: 0.25rem; }
  .status { font-size: 0.75rem; padding: 0.125rem 0.5rem; border-radius: 4px; background: #fef3c7; color: #92400e; }
  .status[data-status="connected"] { background: #d1fae5; color: #065f46; }
  .status[data-status="disconnected"] { background: #fee2e2; color: #991b1b; }
</style>
