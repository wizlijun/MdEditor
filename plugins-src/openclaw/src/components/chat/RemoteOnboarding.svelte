<!-- src/components/chat/RemoteOnboarding.svelte -->
<script lang="ts">
  import { pairClaim } from '../../lib/openclaw/pair'
  import { t } from '../../lib/strings'
  import { describeError } from '../../lib/errors'

  let { onComplete }: { onComplete: () => void } = $props()
  let code = $state('')
  let hostname = $state('')
  let busy = $state(false)
  let err: string | null = $state(null)

  async function submit() {
    if (busy || code.length < 23) return
    busy = true; err = null
    // Trim guards against trailing whitespace from paste — common UX paper-cut.
    const trimmed = code.trim()
    try {
      await pairClaim(trimmed, hostname || undefined)
      onComplete()
    } catch (e) {
      err = describeError(String(e))
    } finally { busy = false }
  }
</script>

<form class="onboard" aria-busy={busy} onsubmit={(e) => { e.preventDefault(); void submit() }}>
  <h2>{t('chat.connectTitle')}</h2>
  <p id="pairing-hint">{t('chat.enterPairingCode')}</p>
  <label>{t('chat.pairingCode')}
    <input bind:value={code} aria-describedby="pairing-hint" placeholder="abc-def-012-345-678-9ab" autocomplete="off" autocapitalize="none" spellcheck="false" disabled={busy} />
  </label>
  <label>{t('chat.deviceNameOptional')}
    <input bind:value={hostname} placeholder="my-laptop" disabled={busy} />
  </label>
  {#if err}<p class="err" role="alert">{err}</p>{/if}
  <button disabled={busy || code.length < 23} type="submit">{busy ? t('chat.connecting') : t('chat.pair')}</button>
</form>

<style>
  .onboard { max-width: 400px; box-sizing: border-box; margin: 24px auto; padding: 24px; border: 1px solid var(--ui-separator); border-radius: 14px; background: var(--ui-surface); }
  h2 { margin: 0 0 10px; font-size: 20px; }
  p { font-size: 13px; line-height: 1.5; color: var(--ui-secondary); }
  label { display: block; margin: 18px 0; font-weight: 600; }
  input { display: block; width: 100%; margin-top: 6px; padding: 8px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-bg); color: CanvasText; font-weight: 400; }
  .err { color: var(--ui-danger); overflow-wrap: anywhere; }
  button { width: 100%; padding: 8px 12px; background: var(--ui-accent); color: var(--ui-accent-foreground); border: 1px solid var(--ui-accent); border-radius: 7px; cursor: pointer; }
  button:disabled { opacity: 0.5; }
  @media (max-width: 440px) { .onboard { margin: 16px 12px; padding: 20px; } }
</style>
