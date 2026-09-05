<!-- src/components/chat/PairingDialog.svelte -->
<script lang="ts">
  import { onMount } from 'svelte'
  import { modalFocus } from '../../../../../src/lib/ui/modal-focus'
  import { pairCreate, type PairCreateOut } from '../../lib/openclaw/pair'
  import { t } from '../../lib/strings'
  import { describeError } from '../../lib/errors'

  let { onClose }: { onClose: () => void } = $props()
  let data: PairCreateOut | null = $state(null)
  let err: string | null = $state(null)
  let busy = $state(false)
  let remaining = $state(120)
  let timer: ReturnType<typeof setInterval> | null = null

  async function create() {
    if (busy) return
    busy = true
    err = null
    try {
      data = await pairCreate()
      remaining = Math.max(0, Math.floor((data.expires_at - Date.now()) / 1000))
      if (timer) clearInterval(timer)
      timer = setInterval(() => {
        remaining = Math.max(0, remaining - 1)
        if (remaining === 0 && timer) clearInterval(timer)
      }, 1000)
    } catch (e) { err = describeError(String(e)) }
    finally { busy = false }
  }

  onMount(() => { void create(); return () => { if (timer) clearInterval(timer) } })
</script>

<div class="overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget && !busy) onClose() }}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="pairing-title" tabindex="-1" use:modalFocus={{ onClose, canClose: () => !busy }}>
    <h2 id="pairing-title">{t('chat.addDevice')}</h2>
    {#if err}
      <p class="err" role="alert">{err}</p>
      <button type="button" disabled={busy} onclick={create}>{t('chat.retry')}</button>
    {:else if busy || !data}
      <p role="status">{t('chat.generatingCode')}</p>
    {:else}
      <div class="qr">{@html data.qr_svg}</div>
      <p class="code">{data.code}</p>
      <p class="hint">{t('chat.expiresIn', { time: `${String(Math.floor(remaining/60)).padStart(2,'0')}:${String(remaining%60).padStart(2,'0')}` })}</p>
      {#if remaining === 0}<button type="button" onclick={create}>{t('chat.retry')}</button>{/if}
    {/if}
    <button type="button" disabled={busy} onclick={onClose}>{t('common.cancel')}</button>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); display: grid; place-items: center; padding: 16px; z-index: 1000; }
  .dialog { box-sizing: border-box; background: var(--ui-surface); color: CanvasText; padding: 24px; border: 1px solid var(--ui-separator); border-radius: 14px; width: min(420px, 100%); max-height: calc(100dvh - 32px); overflow: auto; text-align: center; box-shadow: 0 16px 48px rgb(0 0 0 / 0.2); }
  h2 { margin: 0 0 14px; font-size: 20px; }
  .qr { width: fit-content; max-width: 100%; margin: auto; background: white; color: black; padding: 8px; border-radius: 8px; }
  .qr :global(svg) { width: 220px; max-width: 100%; height: auto; }
  .code { font-family: ui-monospace, monospace; font-size: 18px; letter-spacing: 0.04em; margin: 10px 0; overflow-wrap: anywhere; user-select: all; }
  .hint { color: var(--ui-secondary); font-size: 13px; }
  .err { color: var(--ui-danger); overflow-wrap: anywhere; }
  button { margin: 10px 4px 0; min-height: 32px; padding: 6px 12px; border: 1px solid var(--ui-control-border); background: var(--ui-surface); color: CanvasText; border-radius: 7px; cursor: pointer; }
  button:hover { background: var(--ui-hover); }
  button:disabled { opacity: 0.5; }
</style>
