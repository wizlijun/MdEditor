<script lang="ts">
  import { describeDelta, describeMetadataDelta } from './domain'
  import type { MemoryEntry, Proposal } from './types'

  let { proposal, entries, action, busy = false, oncancel, onconfirm } = $props<{
    proposal: Proposal
    entries: MemoryEntry[]
    action: 'approve' | 'reject'
    busy?: boolean
    oncancel: () => void
    onconfirm: () => void
  }>()

  const delta = $derived(describeDelta(proposal, entries))
  const metadata = $derived(describeMetadataDelta(proposal, entries))
</script>

<div class="scrim" role="presentation" onclick={(event) => event.target === event.currentTarget && !busy && oncancel()}>
  <div class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-description" tabindex="-1">
    <header>
      <span class:reject={action === 'reject'} class="symbol" aria-hidden="true">{action === 'approve' ? '✓' : '×'}</span>
      <div>
        <h2 id="confirm-title">{action === 'approve' ? '确认这项记忆变更' : '拒绝这个候选'}</h2>
        <p id="confirm-description">决定会绑定下列候选 ID、内容与 SHA-256，之后不可静默改写。</p>
      </div>
    </header>

    <div class="identity"><code>{proposal.proposal.id}</code><code>SHA-256 {proposal.sha256}</code></div>
    <div class="diff">
      <div><small>当前</small><p>{delta.before}</p></div>
      <div><small>决定后</small><p>{delta.after}</p></div>
    </div>
    <dl>
      <div><dt>优先级</dt><dd><del>{metadata.priority.before}</del><span>→</span><ins>{metadata.priority.after}</ins></dd></div>
      <div><dt>方向</dt><dd><del>{metadata.polarity.before}</del><span>→</span><ins>{metadata.polarity.after}</ins></dd></div>
      <div><dt>证据性质</dt><dd><del>{metadata.epistemicStatus.before}</del><span>→</span><ins>{metadata.epistemicStatus.after}</ins></dd></div>
      <div><dt>确定度</dt><dd><del>{metadata.certainty.before}</del><span>→</span><ins>{metadata.certainty.after}</ins></dd></div>
      <div class="wide"><dt>Agent 使用规则</dt><dd><del>{metadata.agentGuidance.before}</del><span>→</span><ins>{metadata.agentGuidance.after}</ins></dd></div>
      <div class="wide"><dt>必须避免</dt><dd><del>{metadata.avoidError.before}</del><span>→</span><ins>{metadata.avoidError.after}</ins></dd></div>
    </dl>
    <footer>
      <button class="secondary" type="button" onclick={oncancel} disabled={busy}>取消</button>
      <button class:destructive={action === 'reject'} class="primary" type="button" onclick={onconfirm} disabled={busy}>
        {busy ? '正在提交…' : action === 'approve' ? '确认并写入' : '确认拒绝'}
      </button>
    </footer>
  </div>
</div>

<style>
  .scrim { position:fixed; inset:0; z-index:100; display:grid; place-items:center; padding:24px; background:rgba(0,0,0,.28); backdrop-filter:blur(8px); }
  .sheet { box-sizing:border-box; width:min(620px, 100%); max-height:calc(100vh - 48px); overflow:auto; padding:22px; border:1px solid color-mix(in srgb, CanvasText 15%, transparent); border-radius:14px; background:Canvas; color:CanvasText; box-shadow:0 24px 70px rgba(0,0,0,.34); }
  header { display:flex; gap:12px; align-items:flex-start; } h2 { margin:0; font-size:17px; line-height:1.25; letter-spacing:-.01em; } header p { margin:4px 0 0; color:color-mix(in srgb, CanvasText 62%, transparent); font-size:12px; line-height:1.45; }
  .symbol { display:grid; flex:0 0 28px; width:28px; height:28px; place-items:center; border-radius:50%; background:#34c759; color:white; font-size:18px; font-weight:700; }.symbol.reject { background:#ff3b30; }
  .identity { display:grid; gap:4px; margin:16px 0 12px; padding:10px 12px; border-radius:8px; background:color-mix(in srgb, CanvasText 5%, Canvas); font-size:11px; overflow-wrap:anywhere; }
  .diff { display:grid; grid-template-columns:1fr 1fr; gap:10px; }.diff>div { min-height:72px; padding:11px; border:1px solid color-mix(in srgb, CanvasText 10%, transparent); border-radius:9px; }.diff small,dt { color:color-mix(in srgb, CanvasText 55%, transparent); font-size:11px; }.diff p { margin:5px 0 0; font-size:13px; line-height:1.48; white-space:pre-wrap; }
  dl { display:grid; grid-template-columns:1fr 1fr; gap:8px 18px; margin:14px 0 0; }dl div { display:flex; justify-content:space-between; gap:12px; }dl .wide { grid-column:1/-1; }dd { display:flex; gap:6px; margin:0; font-size:12px; font-weight:600; text-align:right; overflow-wrap:anywhere; }dd del { color:color-mix(in srgb, CanvasText 45%, transparent); font-weight:400; }dd ins { text-decoration:none; }
  footer { display:flex; justify-content:flex-end; gap:8px; margin-top:20px; }button { min-height:34px; padding:0 16px; border:1px solid color-mix(in srgb, CanvasText 16%, transparent); border-radius:8px; font:600 13px -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif; cursor:pointer; pointer-events:auto; -webkit-app-region:no-drag; }button:focus-visible { outline:3px solid color-mix(in srgb, #0a84ff 35%, transparent); outline-offset:2px; }button:disabled { opacity:.45; cursor:default; }.secondary { background:color-mix(in srgb, CanvasText 6%, Canvas); color:CanvasText; }.primary { background:#0a84ff; border-color:#0a84ff; color:white; }.destructive { background:#ff3b30; border-color:#ff3b30; }
  @media (max-width:600px) { .diff,dl { grid-template-columns:1fr; } }
</style>
