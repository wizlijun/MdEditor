<!-- src/components/chat/SessionPicker.svelte -->
<script lang="ts">
  import { t, type MessageKey } from '../../lib/strings'
  import { describeError } from '../../lib/errors'
  import { state as clientState, newSession, openSession } from '../../lib/openclaw/client.svelte'
  let busy = $state(false)
  let error = $state('')
  async function select(action: () => Promise<void>) {
    busy = true
    error = ''
    try { await action() } catch (e) { error = describeError(String(e)) }
    finally { busy = false }
  }
</script>

<header class="picker">
  <div class="session-controls">
    <select aria-label={t('chat.session')} disabled={busy || clientState.sessions.length === 0} value={clientState.currentSessionId ?? ''} onchange={(e) => { const id = e.currentTarget.value; void select(() => openSession(id)) }}>
      {#each clientState.sessions as s (s.id)}<option value={s.id}>{s.title ?? s.id}</option>{/each}
    </select>
    <button type="button" disabled={busy} onclick={() => select(() => newSession())}>{t('chat.newSession')}</button>
    <span class="status" data-status={clientState.status} role="status">{t(('chat.status.' + clientState.status) as MessageKey)}</span>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</header>

<style>
  .picker { flex: none; padding: 12px; border-bottom: 1px solid var(--ui-separator); background: var(--ui-surface); }
  .session-controls { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; }
  select { flex: 1 1 120px; min-width: 0; }
  select, button { padding: 6px 9px; min-height: 32px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: CanvasText; }
  button { cursor: pointer; }
  button:hover { background: var(--ui-hover); }
  .status { font-size: 12px; padding: 3px 7px; border-radius: 6px; background: color-mix(in srgb, var(--ui-warning) 9%, var(--ui-surface)); color: var(--ui-warning); }
  .status[data-status="connected"] { background: color-mix(in srgb, var(--ui-success) 9%, var(--ui-surface)); color: var(--ui-success); }
  .status[data-status="disconnected"] { background: color-mix(in srgb, var(--ui-danger) 9%, var(--ui-surface)); color: var(--ui-danger); }
  .error { color: var(--ui-danger); overflow-wrap: anywhere; font-size: 12px; margin: 8px 0 0; }
</style>
