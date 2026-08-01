<!-- src/components/chat/MessageBubble.svelte -->
<script lang="ts">
  import type { Message } from '../../lib/openclaw/protocol'
  import { openVaultLink } from '../../lib/openclaw/links'
  import { state } from '../../lib/openclaw/client.svelte'
  import { t, type MessageKey } from '../../lib/strings'

  let { message }: { message: Message } = $props()

  function renderText(t: string): { html: string } {
    const escaped = t.replace(/[&<>]/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;'} as Record<string,string>)[c])
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
      currentSession: state.currentSessionId,
      autoSync: true,
    }
  }

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement
    const a = target.closest('a[data-link]') as HTMLAnchorElement | null
    if (!a) return
    e.preventDefault()
    const href = a.getAttribute('href') ?? ''
    openVaultLink(href, getOpts())
  }
</script>

<div class="bubble" class:user={message.role === 'user'} class:agent={message.role === 'agent'} onclick={onClick}>
  <div class="role">{t(('chat.role.' + message.role) as MessageKey)}</div>
  <div class="text">{@html renderText(message.text).html}{#if message.streaming}<span class="cursor">▍</span>{/if}</div>
</div>

<style>
  .bubble { padding: 0.5rem 0.75rem; margin: 0.25rem 0; border-radius: 8px; }
  .bubble.user { background: #2563eb; color: white; align-self: flex-end; max-width: 80%; margin-left: auto; }
  .bubble.agent { background: #f3f4f6; color: #111; max-width: 80%; }
  .role { font-size: 0.7rem; opacity: 0.6; text-transform: uppercase; }
  .text { white-space: pre-wrap; word-break: break-word; }
  .cursor { animation: blink 1s steps(1) infinite; opacity: 0.5; }
  @keyframes blink { 50% { opacity: 0; } }
</style>
