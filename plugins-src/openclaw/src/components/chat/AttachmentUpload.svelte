<!-- src/components/chat/AttachmentUpload.svelte -->
<script lang="ts">
  import { request } from '../../lib/bridge'
  import { state as clientState } from '../../lib/openclaw/client.svelte'
  import { t } from '../../lib/strings'

  let busy = $state(false)

  async function onChange(e: Event) {
    const input = e.target as HTMLInputElement
    const file = input.files?.[0]
    if (!file || !clientState.currentSessionId) return
    busy = true
    try {
      const buf = await file.arrayBuffer()
      const bytes = new Uint8Array(buf)
      // base64 encode via manual loop to avoid call-stack overflow on large files
      let binary = ''
      for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i])
      const b64 = btoa(binary)
      await request('upload_attachment', { session: clientState.currentSessionId, filename: file.name, bytes_b64: b64 })
    } finally { busy = false; input.value = '' }
  }
</script>

<label class="attach" class:busy title={t('chat.attach')} aria-label={t('chat.attach')}>
  <input type="file" onchange={onChange} disabled={busy} hidden />
  📎
</label>

<style>
  .attach { cursor: pointer; padding: 0 0.5rem; font-size: 1.25rem; display: inline-flex; align-items: center; }
  .attach.busy { opacity: 0.5; cursor: progress; }
</style>
