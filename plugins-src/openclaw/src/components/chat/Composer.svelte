<!-- src/components/chat/Composer.svelte -->
<script lang="ts">
  import { tick } from 'svelte'
  import { sendUserMessage } from '../../lib/openclaw/client.svelte'
  import AttachmentUpload from './AttachmentUpload.svelte'
  import { t } from '../../lib/strings'
  import { describeError } from '../../lib/errors'

  let { text = $bindable('') }: { text?: string } = $props()
  let sending = $state(false)
  let error = $state('')
  let textarea: HTMLTextAreaElement
  let sendButton: HTMLButtonElement

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (!text.trim() || sending) return
    const restoreFocus = document.activeElement === textarea || document.activeElement === sendButton
    sending = true
    const payload = text
    error = ''
    try {
      await sendUserMessage(payload)
      if (text === payload) text = ''
    } catch (e) {
      error = describeError(String(e))
    } finally {
      sending = false
      await tick()
      if (restoreFocus && textarea.isConnected && document.activeElement === document.body) textarea.focus()
    }
  }
</script>

<form class="composer" onsubmit={submit} aria-busy={sending}>
  {#if error}<p class="error" role="alert">{t('chat.sendFailed')}: {error}</p>{/if}
  <div class="controls">
    <AttachmentUpload />
    <textarea
      bind:this={textarea}
      bind:value={text}
      aria-label={t('chat.typeToOpenClaw')}
      aria-describedby="composer-shortcut"
      placeholder={t('chat.typeToOpenClaw')}
      rows="2"
      disabled={sending}
      onkeydown={(e) => { if (!e.isComposing && e.keyCode !== 229 && e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); void submit(new SubmitEvent('submit')) } }}
    ></textarea>
    <button bind:this={sendButton} class="send" type="submit" disabled={!text.trim() || sending}>{sending ? t('chat.sending') : t('chat.send')}</button>
  </div>
  <p id="composer-shortcut" class="hint">{t('chat.sendShortcut')}</p>
</form>

<style>
  .composer { flex: none; padding: 10px 12px; border-top: 1px solid var(--ui-separator); background: var(--ui-surface); }
  .controls { display: flex; align-items: flex-end; gap: 8px; min-width: 0; }
  textarea { flex: 1; min-width: 0; resize: vertical; min-height: 58px; max-height: 30vh; padding: 8px; border: 1px solid var(--ui-control-border); border-radius: 7px; background: var(--ui-bg); color: CanvasText; font: inherit; }
  .send { flex: none; min-height: 34px; padding: 6px 12px; border: 1px solid var(--ui-accent); border-radius: 7px; background: var(--ui-accent); color: var(--ui-accent-foreground); cursor: pointer; }
  .send:disabled { opacity: 0.5; cursor: default; }
  .hint { margin: 6px 0 0; color: var(--ui-secondary); font-size: 12px; }
  .error { margin: 0 0 8px; color: var(--ui-danger); font-size: 13px; overflow-wrap: anywhere; }
</style>
