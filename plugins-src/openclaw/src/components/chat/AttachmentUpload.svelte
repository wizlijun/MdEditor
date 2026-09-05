<!-- src/components/chat/AttachmentUpload.svelte -->
<script lang="ts">
  import { request } from '../../lib/bridge'
  import { state as clientState } from '../../lib/openclaw/client.svelte'
  import { t } from '../../lib/strings'
  import { describeError } from '../../lib/errors'

  let busy = $state(false)
  let error = $state('')
  let fileInput: HTMLInputElement

  async function onChange(e: Event) {
    const input = e.target as HTMLInputElement
    const file = input.files?.[0]
    if (!file || !clientState.currentSessionId) return
    busy = true
    error = ''
    try {
      const buf = await file.arrayBuffer()
      const bytes = new Uint8Array(buf)
      // base64 encode via manual loop to avoid call-stack overflow on large files
      let binary = ''
      for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i])
      const b64 = btoa(binary)
      await request('upload_attachment', { session: clientState.currentSessionId, filename: file.name, bytes_b64: b64 })
    } catch (e) { error = describeError(String(e)) }
    finally { busy = false; input.value = '' }
  }
</script>

<div class="attachment">
  <input bind:this={fileInput} type="file" onchange={onChange} disabled={busy} hidden />
  <button class="attach" type="button" disabled={busy || !clientState.currentSessionId} title={t('chat.attach')} aria-label={t('chat.attach')} aria-busy={busy} onclick={() => fileInput.click()}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="m8 12 6-6a3 3 0 0 1 4 4l-8 8a5 5 0 0 1-7-7l9-9a2 2 0 0 1 3 3l-9 9a1 1 0 0 0 2 2l8-8" /></svg>
  </button>
  {#if error}<p class="error" role="alert">{t('chat.uploadFailed')}: {error}</p>{/if}
</div>

<style>
  .attachment { flex: none; }
  .attach { display: grid; place-items: center; width: 34px; height: 34px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-surface); color: var(--ui-secondary); cursor: pointer; }
  .attach:hover { background: var(--ui-hover); }
  .attach:disabled { opacity: 0.5; cursor: default; }
  svg { width: 18px; height: 18px; }
  .error { max-width: 130px; color: var(--ui-danger); font-size: 12px; overflow-wrap: anywhere; margin: 6px 0 0; }
</style>
