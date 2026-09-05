<!-- src/components/chat/MessageBubble.svelte -->
<script lang="ts">
  import type { Message } from '../../lib/openclaw/protocol'
  import { openVaultLink } from '../../lib/openclaw/links'
  import { state as clientState } from '../../lib/openclaw/client.svelte'
  import { describeError } from '../../lib/errors'
  import { t, type MessageKey } from '../../lib/strings'

  let { message }: { message: Message } = $props()
  let linkError = $state('')

  function renderText(t: string): { html: string } {
    const escaped = t.replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'} as Record<string,string>)[c])
    const linked = escaped.replace(
      /\[([^\]]+)\]\(([^)]+)\)/g,
      (_m, label, href) => `<a href="${href}" data-link>${label}</a>`
    )
    return { html: linked.replace(/\n/g, '<br>') }
  }

  // Get vault root + auto-sync from settings — fall back to defaults until P2.9 adds the settings tab.
  function getOpts() {
    // TODO P2.9: read from settings store. For now: best-effort lookup from existing vault_sync repo_path.
    return {
      vaultRoot: null as string | null,
      isBoundMode: false,
      currentSession: clientState.currentSessionId,
      autoSync: true,
    }
  }

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement
    const a = target.closest('a[data-link]') as HTMLAnchorElement | null
    if (!a) return
    e.preventDefault()
    const href = a.getAttribute('href') ?? ''
    linkError = ''
    void openVaultLink(href, getOpts()).catch((error) => { linkError = describeError(String(error)) })
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions (Delegation only: generated native anchors already support Enter and Tab.) -->
<article class="bubble" class:user={message.role === 'user'} class:agent={message.role === 'agent'} onclick={onClick}>
  <div class="role">{t(('chat.role.' + message.role) as MessageKey)}</div>
  <div class="text">{@html renderText(message.text).html}{#if message.streaming}<span class="cursor">▍</span>{/if}</div>
  {#if linkError}<p class="error" role="alert">{linkError}</p>{/if}
</article>

<style>
  .bubble { padding: 10px 12px; margin: 4px 0; border-radius: 10px; border: 1px solid var(--ui-separator); min-width: 0; max-width: 90%; background: var(--ui-surface); color: CanvasText; }
  .bubble.user { background: var(--ui-selection); align-self: flex-end; margin-left: auto; }
  .bubble.agent { align-self: flex-start; }
  .role { font-size: 12px; color: var(--ui-secondary); font-weight: 600; margin-bottom: 4px; }
  .text { white-space: pre-wrap; overflow-wrap: anywhere; }
  .text :global(a) { color: var(--ui-accent-text); text-underline-offset: 2px; }
  .error { color: var(--ui-danger); font-size: 12px; overflow-wrap: anywhere; }
  .cursor { animation: blink 1s steps(1) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .cursor { animation: none; } }
</style>
