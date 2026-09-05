<!-- src/components/chat/PendingClaimToast.svelte -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { describeError } from '../../lib/errors'
  import { t } from '../../lib/strings'
  import { onPendingClaim, approveClaim, rejectClaim, type PendingClaim } from '../../lib/openclaw/pair'
  import { refresh } from '../../lib/openclaw/devices.svelte'

  let pending = $state<PendingClaim[]>([])
  let busy = $state(new Set<string>())
  let errors = $state<Record<string, string>>({})

  onMount(() => {
    // v2 onPendingClaim returns the unsubscribe fn synchronously (v1 returned a
    // Promise from listen()).
    const unsub = onPendingClaim((c) => { pending = [...pending, c] })
    return () => { unsub() }
  })

  async function decide(c: PendingClaim, allowed: boolean) {
    if (busy.has(c.device_id)) return
    busy = new Set([...busy, c.device_id])
    errors = { ...errors, [c.device_id]: '' }
    try {
      if (allowed) await approveClaim(c.device_id, c.hostname)
      else await rejectClaim(c.device_id)
      await refresh()
      pending = pending.filter((p) => p.device_id !== c.device_id)
    } catch (e) { errors = { ...errors, [c.device_id]: describeError(String(e)) } }
    finally { busy = new Set([...busy].filter((id) => id !== c.device_id)) }
  }
</script>

{#if pending.length > 0}
  <section class="claims" aria-label={t('chat.newDeviceWantsToConnect')}>
    {#each pending as c (c.device_id)}
      <div class="toast" aria-busy={busy.has(c.device_id)}>
        <p class="request">{t('chat.newDeviceWantsToConnect')} <b>{c.hostname}</b></p>
        {#if errors[c.device_id]}<p class="error" role="alert">{errors[c.device_id]}</p>{/if}
        <div class="actions">
          <button type="button" disabled={busy.has(c.device_id)} onclick={() => decide(c, false)}>{t('chat.reject')}</button>
          <button class="allow" type="button" disabled={busy.has(c.device_id)} onclick={() => decide(c, true)}>{t('chat.allow')}</button>
        </div>
      </div>
    {/each}
  </section>
{/if}

<style>
  .claims { position: fixed; right: 12px; top: 12px; display: grid; gap: 8px; width: min(340px, calc(100vw - 24px)); max-height: calc(60dvh - 24px); overflow: auto; z-index: 900; padding: 3px; box-sizing: border-box; }
  .toast { background: var(--ui-surface); color: CanvasText; padding: 14px; border: 1px solid var(--ui-separator); border-radius: 12px; box-shadow: 0 4px 18px rgb(0 0 0 / 0.16); overflow-wrap: anywhere; }
  .request { margin: 0; font-size: 13px; line-height: 1.5; }
  .actions { margin-top: 12px; display: flex; flex-wrap: wrap; gap: 8px; justify-content: flex-end; }
  button { padding: 6px 12px; min-height: 32px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: CanvasText; cursor: pointer; }
  button:hover { background: var(--ui-hover); }
  .allow { border-color: var(--ui-accent); background: var(--ui-accent); color: var(--ui-accent-foreground); }
  .allow:hover { background: var(--ui-accent); }
  button:disabled { opacity: 0.5; }
  .error { margin: 8px 0 0; font-size: 12px; color: var(--ui-danger); }
</style>
