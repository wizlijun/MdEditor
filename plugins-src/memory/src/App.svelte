<script lang="ts">
  import { onMount } from 'svelte'
  import { bridge, memoryDecide, memoryList, memoryMigrate, memoryPropose, memorySuggest, toast } from './lib/bridge'
  import { describeDelta, exactDecisionPrompt, filterEntries, pendingProposals } from './lib/domain'
  import { setLocale, t } from './lib/strings'
  import type { MemoryEntry, Operation, Priority, Proposal, Scope, Snapshot } from './lib/types'

  setLocale(bridge().locale)

  let snapshot = $state<Snapshot | null>(null)
  let busy = $state(false)
  let error = $state('')
  let tab = $state<'current' | 'pending' | 'improve'>('current')
  let query = $state('')
  let scope = $state<'all' | Scope>('all')
  let status = $state('active')
  let highOnly = $state(false)
  let suggestions = $state<unknown[]>([])
  let editId = $state<string | null>(null)
  let formScope = $state<Scope>('memory')
  let formText = $state('')
  let formSource = $state('')
  let formSection = $state('Active memory')
  let ownerActor = $state('human:')
  let ownerNames = $state('')

  const visibleEntries = $derived(snapshot ? filterEntries(snapshot.entries, query, scope, status, highOnly) : [])
  const reviews = $derived(snapshot ? pendingProposals(snapshot.proposals) : [])

  async function refresh() {
    busy = true; error = ''
    try { snapshot = await memoryList() }
    catch (e) { error = String(e) }
    finally { busy = false }
  }

  function humanSource(): string {
    return `human-input://${snapshot?.owner_actor ?? 'unknown'}/${new Date().toISOString()}`
  }

  function newEntry() {
    editId = 'new'; formScope = 'memory'; formText = ''; formSource = humanSource(); formSection = 'Active memory'
  }

  function edit(entry: MemoryEntry) {
    editId = entry.id; formScope = entry.scope; formText = entry.text; formSource = entry.source ?? humanSource(); formSection = entry.section
  }

  async function proposeAndApprove(input: {
    scope: Scope; operation: Operation; text: string; source: string; target?: MemoryEntry; priority?: Priority; section?: string
  }) {
    if (!snapshot?.owner_actor) throw new Error('USER.md has no confirmed owner actor')
    const nonce = crypto.randomUUID()
    const proposal = await memoryPropose({
      scope: input.scope, operation: input.operation, text: input.text, source: input.source || humanSource(),
      by: 'notemd-memory/human-ui', dedupe_key: `memory-ui/v1/${nonce}`,
      reason: 'Direct human change through the controlled Memory window.',
      target_id: input.target?.id, base_revision: input.target?.revision, section: input.section,
      priority: input.priority, merge_from: [],
    })
    if (!window.confirm(exactDecisionPrompt(proposal, snapshot.entries, t('confirmDecision')))) {
      await refresh()
      return
    }
    await memoryDecide({ proposal_id: proposal.proposal.id, expected_sha256: proposal.sha256,
      action: 'approve', actor: snapshot.owner_actor,
      human_confirmed: true, reason: 'Approved through the Memory window.' })
    await toast('success', t('saveApprove'))
    editId = null
    await refresh()
  }

  async function submitForm() {
    if (!formText.trim()) return
    const target = editId === 'new' ? undefined : snapshot?.entries.find((entry) => entry.id === editId)
    busy = true; error = ''
    try {
      await proposeAndApprove({ scope: formScope, operation: target ? 'replace' : 'create', text: formText,
        source: formSource, target, section: formSection, priority: target?.priority ?? 'normal' })
    } catch (e) { error = String(e) } finally { busy = false }
  }

  async function changeStatus(entry: MemoryEntry) {
    busy = true; error = ''
    try {
      await proposeAndApprove({ scope: entry.scope, operation: entry.status === 'revoked' ? 'replace' : 'revoke',
        text: entry.text, source: entry.source ?? humanSource(), target: entry, priority: entry.priority })
    } catch (e) { error = String(e) } finally { busy = false }
  }

  async function togglePriority(entry: MemoryEntry) {
    busy = true; error = ''
    try {
      await proposeAndApprove({ scope: entry.scope, operation: 'set-priority', text: '', source: entry.source ?? humanSource(),
        target: entry, priority: entry.priority === 'high' ? 'normal' : 'high' })
    } catch (e) { error = String(e) } finally { busy = false }
  }

  async function decideProposal(proposal: Proposal, action: 'approve' | 'reject') {
    if (!snapshot?.owner_actor) return
    const heading = action === 'approve' ? t('confirmDecision') : t('confirmReject')
    const message = exactDecisionPrompt(proposal, snapshot.entries, heading)
    if (!window.confirm(message)) return
    busy = true; error = ''
    try {
      await memoryDecide({ proposal_id: proposal.proposal.id, expected_sha256: proposal.sha256,
        action, actor: snapshot.owner_actor,
        human_confirmed: true, reason: `Human ${action} through Memory window.` })
      await refresh()
    } catch (e) { error = String(e) } finally { busy = false }
  }

  async function migrate() {
    if (!window.confirm(t('confirmMigrate'))) return
    busy = true; error = ''
    try { const result = await memoryMigrate(); await toast('success', `${result.migrated} entries imported`); await refresh() }
    catch (e) { error = String(e) } finally { busy = false }
  }

  async function claimOwner() {
    const names = ownerNames.split(',').map((name) => name.trim()).filter(Boolean)
    if (!ownerActor.startsWith('human:') || names.length === 0) return
    busy = true; error = ''
    try {
      const proposal = await memoryPropose({
        scope: 'user-owner', operation: 'create', text: JSON.stringify({ actor: ownerActor, names }, null, 2),
        source: humanSource(), by: 'notemd-memory/human-ui', dedupe_key: `memory-ui/v1/owner/${crypto.randomUUID()}`,
        reason: 'Initial owner claim through the controlled Memory window.', section: 'Owner', priority: 'high', merge_from: [],
      })
      if (!window.confirm(exactDecisionPrompt(proposal, snapshot?.entries ?? [], t('confirmDecision')))) {
        await refresh()
        return
      }
      await memoryDecide({ proposal_id: proposal.proposal.id, expected_sha256: proposal.sha256,
        action: 'approve', actor: ownerActor,
        human_confirmed: true, reason: 'Owner confirmed through the Memory window.' })
      await refresh()
    } catch (e) { error = String(e) } finally { busy = false }
  }

  async function loadSuggestions() {
    busy = true; error = ''
    try { suggestions = (await memorySuggest()).suggestions ?? [] }
    catch (e) { error = String(e) } finally { busy = false }
  }

  onMount(refresh)
</script>

<svelte:head><title>{t('title')}</title></svelte:head>

<main>
  <header>
    <div><h1>{t('title')}</h1><p>{t('subtitle')}</p></div>
    <button class="quiet" onclick={refresh} disabled={busy}>{t('refresh')}</button>
  </header>

  <div class="readonly">🔒 {t('directReadOnly')}</div>

  {#if error}<div class="banner error">{error}</div>{/if}
  {#if snapshot?.integrity.drift}<div class="banner error"><strong>{t('drift')}</strong><br />{snapshot.integrity.errors.join(' · ')}</div>{/if}
  {#if snapshot && !snapshot.integrity.managed}
    <div class="banner migrate"><div><strong>{t('migrate')}</strong><p>{t('migrateHint')}</p></div><button onclick={migrate} disabled={busy}>{t('migrate')}</button></div>
  {/if}
  {#if snapshot?.integrity.managed && !snapshot.owner_actor}
    <div class="banner migrate owner-claim">
      <strong>{t('claimOwner')}</strong>
      <input bind:value={ownerActor} placeholder={t('ownerActor')} />
      <input bind:value={ownerNames} placeholder={t('ownerNames')} />
      <button class="primary" onclick={claimOwner} disabled={busy}>{t('claimOwner')}</button>
    </div>
  {/if}

  <nav aria-label="Memory sections">
    <button class:active={tab === 'current'} onclick={() => tab = 'current'}>{t('current')}</button>
    <button class:active={tab === 'pending'} onclick={() => tab = 'pending'}>{t('pending')} <span class="count">{reviews.length}</span></button>
    <button class:active={tab === 'improve'} onclick={() => tab = 'improve'}>{t('improve')}</button>
  </nav>

  {#if busy && !snapshot}<div class="loading">{t('loading')}</div>{/if}

  {#if tab === 'current' && snapshot}
    <section class="toolbar">
      <input class="search" bind:value={query} placeholder={t('search')} />
      <select bind:value={scope}><option value="all">{t('all')}</option><option value="user-profile">{t('user')}</option><option value="memory">{t('memory')}</option></select>
      <select bind:value={status}><option value="active">{t('active')}</option><option value="pending">{t('pendingState')}</option><option value="revoked">{t('revoked')}</option><option value="all">{t('all')}</option></select>
      <label><input type="checkbox" bind:checked={highOnly} /> {t('highOnly')}</label>
      <button class="primary" onclick={newEntry} disabled={!snapshot.integrity.managed || snapshot.integrity.drift}>{t('add')}</button>
    </section>

    {#if editId}
      <section class="editor">
        <div class="row"><select bind:value={formScope}><option value="user-profile">{t('user')}</option><option value="memory">{t('memory')}</option></select><input bind:value={formSection} placeholder={t('section')} /></div>
        <textarea bind:value={formText} rows="5" placeholder={t('content')}></textarea>
        <input bind:value={formSource} placeholder={t('source')} />
        <div class="actions"><button class="quiet" onclick={() => editId = null}>{t('cancel')}</button><button class="primary" onclick={submitForm} disabled={busy || !formText.trim()}>{t('saveApprove')}</button></div>
      </section>
    {/if}

    <section class="cards">
      {#each visibleEntries as entry (entry.id)}
        <article class:muted={entry.status !== 'active'}>
          <div class="card-head"><span class="scope">{entry.scope === 'memory' ? t('memory') : t('user')}</span><span class:high={entry.priority === 'high'}>{entry.priority === 'high' ? t('high') : t('normal')}</span><span>{entry.status}</span><code>r{entry.revision}</code></div>
          <p class="claim">{entry.text}</p>
          <p class="meta">{entry.section} · {entry.source ?? '—'} · <code>{entry.id}</code></p>
          <div class="actions"><button class="quiet" onclick={() => edit(entry)} disabled={snapshot.integrity.drift}>{t('edit')}</button><button class="quiet" onclick={() => togglePriority(entry)} disabled={snapshot.integrity.drift}>{entry.priority === 'high' ? t('normal') : t('high')}</button><button class="danger" onclick={() => changeStatus(entry)} disabled={snapshot.integrity.drift}>{entry.status === 'revoked' ? t('restore') : t('revoke')}</button></div>
        </article>
      {:else}<div class="empty">{t('noEntries')}</div>{/each}
    </section>
  {/if}

  {#if tab === 'pending' && snapshot}
    <section class="cards review-list">
      {#each reviews as proposal (proposal.proposal.id)}
        {@const delta = describeDelta(proposal, snapshot.entries)}
        <article class:action-sensitive={proposal.proposal.action_sensitive}>
          <div class="card-head"><span class="scope">{proposal.proposal.scope}</span><strong>{proposal.proposal.operation}</strong>{#if proposal.proposal.suggested_priority === 'high'}<span class="high">{t('high')}</span>{/if}</div>
          <div class="diff"><div><small>{t('before')}</small><p>{delta.before}</p></div><div><small>{t('after')}</small><p>{delta.after}</p></div></div>
          <p class="meta">{t('proposedBy')}: {proposal.generated.by} · {t('source')}: {proposal.sources[0]?.resource ?? '—'}</p>
          <p class="meta"><code>{proposal.proposal.id}</code> · SHA-256 <code>{proposal.sha256}</code></p>
          {#if proposal.reason}<p class="reason"><strong>{t('reason')}:</strong> {proposal.reason}</p>{/if}
          <div class="actions"><button class="danger" onclick={() => decideProposal(proposal, 'reject')}>{t('reject')}</button><button class="primary" onclick={() => decideProposal(proposal, 'approve')}>{t('approve')}</button></div>
        </article>
      {:else}<div class="empty">{t('noPending')}</div>{/each}
    </section>
  {/if}

  {#if tab === 'improve'}
    <section class="improve"><button class="primary" onclick={loadSuggestions} disabled={busy}>{t('runSuggest')}</button>
      {#each suggestions as suggestion}<pre>{JSON.stringify(suggestion, null, 2)}</pre>{/each}
    </section>
  {/if}
</main>

<style>
  :global(:root) { color-scheme: light dark; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif; }
  :global(body) { margin: 0; background: Canvas; color: CanvasText; }
  :global(button), :global(input), :global(select), :global(textarea) { font: inherit; }
  main { max-width: 1160px; margin: 0 auto; padding: 22px 26px 48px; }
  header { display:flex; justify-content:space-between; align-items:flex-start; gap:20px; }
  h1 { margin:0; font-size:26px; letter-spacing:-.02em; } header p { margin:5px 0 0; opacity:.62; }
  .readonly { margin:18px 0 12px; padding:8px 12px; border-radius:8px; background:color-mix(in srgb, CanvasText 6%, Canvas); font-size:13px; opacity:.82; }
  .banner { display:flex; align-items:center; justify-content:space-between; gap:20px; margin:12px 0; padding:14px 16px; border-radius:10px; border:1px solid; }
  .banner p { margin:4px 0 0; }.error { border-color:#d74b4b; background:color-mix(in srgb, #d74b4b 10%, Canvas); }.migrate { border-color:#d59b21; background:color-mix(in srgb, #d59b21 10%, Canvas); }
  .owner-claim { display:grid; grid-template-columns:auto 1fr 1fr auto; align-items:center; }
  nav { display:flex; gap:4px; margin:18px 0; border-bottom:1px solid color-mix(in srgb, CanvasText 14%, transparent); }
  nav button { border:0; background:none; padding:10px 14px; opacity:.64; border-bottom:2px solid transparent; } nav button.active { opacity:1; border-color:#0a84ff; }
  .count { display:inline-grid; place-items:center; min-width:18px; height:18px; padding:0 3px; border-radius:9px; font-size:11px; background:color-mix(in srgb, CanvasText 10%, Canvas); }
  button { border:1px solid color-mix(in srgb, CanvasText 18%, transparent); background:color-mix(in srgb, CanvasText 6%, Canvas); border-radius:7px; padding:7px 11px; cursor:pointer; } button:disabled { opacity:.4; cursor:default; }
  button.primary { background:#0a84ff; border-color:#0a84ff; color:white; } button.danger { color:#d93636; } button.quiet { background:transparent; }
  .toolbar { display:flex; flex-wrap:wrap; align-items:center; gap:9px; margin-bottom:14px; }.toolbar .search { flex:1; min-width:220px; }.toolbar label { font-size:13px; opacity:.8; }
  input, select, textarea { box-sizing:border-box; border:1px solid color-mix(in srgb, CanvasText 18%, transparent); background:Canvas; color:CanvasText; border-radius:7px; padding:8px 10px; }
  textarea { width:100%; resize:vertical; line-height:1.5; }.editor { padding:14px; margin-bottom:14px; border:1px solid #0a84ff; border-radius:10px; }.editor .row { display:grid; grid-template-columns:180px 1fr; gap:8px; margin-bottom:8px; }.editor > input { width:100%; margin-top:8px; }
  .cards { display:grid; gap:10px; } article { padding:14px 15px; border:1px solid color-mix(in srgb, CanvasText 15%, transparent); border-radius:11px; background:color-mix(in srgb, CanvasText 2%, Canvas); } article.muted { opacity:.62; } article.action-sensitive { border-color:#e46b45; }
  .card-head { display:flex; align-items:center; gap:8px; font-size:12px; opacity:.76; }.card-head span { padding:2px 6px; border-radius:5px; background:color-mix(in srgb, CanvasText 7%, Canvas); }.card-head .high { background:#ff9f0a; color:#231500; opacity:1; }.scope { color:#0a84ff; }
  .claim { font-size:15px; line-height:1.55; margin:10px 0 8px; }.meta { margin:0; font-size:12px; opacity:.55; overflow-wrap:anywhere; }.reason { font-size:13px; opacity:.76; }
  .actions { display:flex; justify-content:flex-end; gap:7px; margin-top:12px; }.diff { display:grid; grid-template-columns:1fr 1fr; gap:10px; margin-top:12px; }.diff > div { padding:10px; border-radius:8px; background:color-mix(in srgb, CanvasText 5%, Canvas); }.diff small { opacity:.55; }.diff p { margin:5px 0 0; line-height:1.5; }
  .empty,.loading { padding:48px; text-align:center; opacity:.5; }.improve pre { white-space:pre-wrap; padding:12px; border-radius:8px; background:color-mix(in srgb, CanvasText 6%, Canvas); }
  @media (max-width: 760px) { main { padding:16px; }.diff { grid-template-columns:1fr; }.toolbar { align-items:stretch; }.toolbar > * { flex:1; }.editor .row { grid-template-columns:1fr; } }
</style>
