<!-- src/components/chat/PairingDialog.svelte -->
<script lang="ts">
  import { pairCreate, type PairCreateOut } from '../../lib/openclaw/pair'
  import { t } from '../../lib/strings'
  import { describeError } from '../../lib/errors'

  let { onClose }: { onClose: () => void } = $props()
  let data: PairCreateOut | null = $state(null)
  let err: string | null = $state(null)
  let remaining = $state(120)
  let timer: ReturnType<typeof setInterval> | null = null

  async function create() {
    try {
      data = await pairCreate()
      remaining = Math.max(0, Math.floor((data.expires_at - Date.now()) / 1000))
      if (timer) clearInterval(timer)
      timer = setInterval(() => {
        remaining = Math.max(0, remaining - 1)
        if (remaining === 0 && timer) clearInterval(timer)
      }, 1000)
    } catch (e) { err = describeError(String(e)) }
  }

  $effect(() => { create(); return () => { if (timer) clearInterval(timer) } })
</script>

<div class="overlay" onclick={onClose}>
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <h2>{t('chat.addDevice')}</h2>
    {#if err}
      <p class="err">{err}</p>
      <button onclick={create}>{t('chat.retry')}</button>
    {:else if !data}
      <p>{t('chat.generatingCode')}</p>
    {:else}
      <div class="qr">{@html data.qr_svg}</div>
      <p class="code">{data.code}</p>
      <p class="hint">{t('chat.expiresIn', { time: `${String(Math.floor(remaining/60)).padStart(2,'0')}:${String(remaining%60).padStart(2,'0')}` })}</p>
    {/if}
    <button onclick={onClose}>{t('common.cancel')}</button>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 1000; }
  .dialog { background: white; padding: 1.5rem; border-radius: 8px; min-width: 340px; max-width: 460px; text-align: center; }
  .qr :global(svg) { width: 220px; height: 220px; }
  .code { font-family: ui-monospace, monospace; font-size: 1.25rem; letter-spacing: 0.05em; margin: 0.5rem 0; }
  .hint { color: #777; font-size: 0.85rem; }
  .err { color: #b91c1c; }
  button { margin-top: 1rem; padding: 0.4rem 0.8rem; border: 1px solid #d1d5db; background: white; border-radius: 6px; cursor: pointer; }
</style>
