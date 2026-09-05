// Browser-only fixtures. No native IPC, filesystem, account or model requests.
export function installUiReviewBridge(kind) {
  const calls = []
  const callbacks = new Map()
  const stored = new Map([['locale', 'zh']])
  const pushes = []
  const nextFiles = new Map()
  let sequence = 0
  const revision = {
    schema: 'notemd.memory/claim-revision/v2', claim_id: 'claim-1', revision_id: 'revision-1', parents: [],
    claim_kind: 'preference', subject: { kind: 'vault-owner', id: 'owner-1', relation_to_owner: 'self', label: 'Demo' },
    asserted_by: [{ kind: 'owner', id: 'owner-1' }], recorded_by: { kind: 'host', id: 'notemd.memory-ui' },
    recorded_at: '2026-09-01T08:30:00Z', text: '先给出结论，再说明依据。\n不确定的内容应明确标注。',
    projection: { target: 'user', category: 'preferences', visibility: 'projection' }, workflow: { state: 'approved' }, lifecycle: { state: 'active' },
    temporal: { valid_from: '2026-09-01T08:30:00Z' },
    epistemic: { basis: 'owner-stated', representation_certainty: 'high', truth_status: 'not-assessed', truth_confidence: 'unknown' },
    trust_tier: 'stable-preference', risk_class: 'informational', salience: 'normal', polarity: 'positive', sensitivity: 'normal',
    context: { spaces: ['global'], applies_when: [], excludes_when: [] },
    consent: { scope: 'personal-assistant-only', allowed_purposes: ['planning', 'writing'], external_provider_policy: 'prompt' },
    agent_use: { guidance: '先给结论。', avoid_error: '不构成外部行动授权。' },
    decision: { verdict: 'approve', approval_kind: 'self-representation', authority_scope: 'personal-assistant', actor_id: 'human:demo', decided_at: '2026-09-01T08:30:00Z' },
    evidence: [], payload_sha256: 'fixture-sha',
  }
  const snapshot = {
    mode: 'v2', protocol: { revision_id: 'protocol-2', payload_sha256: 'protocol-sha' },
    owner: { actor_id: 'human:demo', subject: revision.subject }, claims: [{ claim: revision, application_state: 'current' }],
    pending: [{ revision: { ...revision, revision_id: 'pending-1', workflow: { state: 'pending' }, decision: undefined }, expected_sha256: 'fixture-sha', expected_heads: [], source_summary: 'notes/review.md' }],
    conflicts: [], history: [], health: { status: 'attention', message: '1 条待确认', pending_count: 1, conflict_count: 0, integrity_errors: [] },
    context_options: { spaces: [{ id: 'global', label: '全局' }], purposes: [{ id: 'planning', label: '规划' }], providers: [{ id: 'openai', label: 'OpenAI' }], models: [{ id: 'example', label: '示例模型', provider_id: 'openai' }] },
  }
  const registry = {
    protocol: snapshot.protocol, registry_heads: [], writable: true,
    roles: [{ id: 'role:developer', label: '开发者', description: '', aliases: [], status: 'active', guidance: '', avoid_error: '' }],
    scopes: [{ id: 'global', label: '全局', description: '', aliases: [], status: 'active', kind: 'realm', security_domain: 'personal' }],
  }
  const market = ['memory', 'codex-agent', 'next', 'meetings', 'ebook-import'].map((name) => ({
    id: 'notemd.' + name, name: name === 'memory' ? 'Memory' : name, version: '9.0.1', min_host: '>=6.0.0', archs: ['universal'], size: 1024,
    category: name === 'meetings' ? 'record' : 'ai', description: '用清晰的文档、可追溯的来源和明确的操作管理日常工作。',
    sha256: { universal: 'fixture' }, download: { universal: 'https://fixture.invalid/plugin.zip' },
  }))
  async function request(method, params = {}) {
    calls.push({ method, params })
    if (kind === 'trace-source' && method === 'host.vault.write' && params.path === '.notemd/trace-source.json' && window.__uiReview.failTraceSettings) throw new Error('Fixture settings write refused')
    if (kind === 'idea-spark' && method === 'host.vault.write' && params.path === '.notemd/idea-spark.json' && window.__uiReview.failIdeaSettings) throw new Error('Fixture settings write refused')
    if (kind === 'next' && method === 'host.vault.exists') return { exists: nextFiles.has(params.path) }
    if (kind === 'next' && method === 'host.vault.write') { nextFiles.set(params.path, params.content); return { ok: true } }
    if (kind === 'next' && method === 'host.vault.read') {
      if (nextFiles.has(params.path)) return { content: nextFiles.get(params.path) }
      throw new Error('ENOENT: no such file or directory')
    }
    if (kind === 'weekly-review' && method === 'host.vault.exists' && params.path === 'weekly-review') return { exists: true }
    if (kind === 'weekly-review' && method === 'host.vault.list' && params.path === 'weekly-review') return { entries: [{ name: '2026-W36-weekly-review.md', is_dir: false }] }
    if (method === 'host.vault.info') return { root: '/fixture-vault', wiki_dir: 'wiki', daily_dir: 'daily', author: 'human:demo' }
    if (method === 'host.vault.exists') return { exists: false }
    if (method === 'host.vault.list') return { entries: [] }
    if (method === 'host.vault.read') throw new Error('Fixture file does not exist')
    if (method === 'host.settings.get') return { settings: { maxConcurrency: '2', usageDisplay: 'tip', ...Object.fromEntries(stored) } }
    if (method === 'host.settings.set') { Object.entries(params.settings ?? {}).forEach(([key, value]) => stored.set(key, value)); return {} }
    if (method === 'host.agent.providers') return { providers: [] }
    if (method === 'host.power_mode.config') return { config: {}, surfaces: [{ id: 'notemd.idea-spark', name: 'Idea Spark', names: { zh: '灵感' } }] }
    if (method === 'host.memory.v2.snapshot') return snapshot
    if (method === 'host.memory.v2.contextRegistry') return registry
    if (method === 'plugin.tasks.list') return { ready: true, tasks: [{ id: 'review', name: '检查项目', description: '检查背景文档与引用。', group: 'custom' }] }
    if (method === 'plugin.history.list') return { runs: [] }
    if (method === 'plugin.context.get') return { tab: { path: 'projects/demo.md', selection: '' } }
    if (method === 'plugin.harness-status') return { ok: true, harness: kind === 'claude-agent' ? 'Claude' : kind === 'deepseek-agent' ? 'DeepSeek' : 'Codex', version: '1.0' }
    if (method === 'plugin.detect_env') return kind === 'meetings'
      ? { settings: { meetings_root: 'ssot/meetings' }, default_hemory_source: null }
      : { ready: true, settings: { ebooks_root: 'ebooks', wechat_url: 'http://fixture.invalid', provider: 'wechat' }, device: { calibre_path: '', baidu_api_key_set: false, baidu_secret_key_set: false }, calibre: null }
    if (method === 'plugin.library_list') return { books: [], meetings: [{ conversation_id: 'demo', title: '项目设计评审', started_at: '2026-09-05T09:00:00+08:00', duration_ms: 1800000, speaker_count: 3, transcript_path: 'ssot/meetings/demo/transcript.md' }] }
    if (method === 'plugin.topic_state') return { revision: 'absent', catalog: null, counts: {} }
    if (method === 'plugin.save_settings') return { meetings_root: params.meetings_root ?? 'ssot/meetings', ok: true }
    if (method === 'plugin.connect') return 'host'
    if (method === 'plugin.send') return {}
    if (method === 'host.cdr.repository.inspect') return { kind: 'empty' }
    if (method === 'host.dialog.open') return { paths: null }
    return {}
  }
  window.notemd = { pluginId: 'notemd.' + kind, locale: 'zh', theme: 'system', request, onMessage: (callback) => { pushes.push(callback) } }
  window.__uiReview = { calls, pushes, snapshot, registry, ready: false, error: null }
  window.__TAURI_OS_PLUGIN_INTERNALS__ = { platform: 'macos', arch: 'aarch64', type: 'macos', locale: 'zh-CN' }
  // Tauri's public unlisten() calls this native-injected hook before IPC.
  // This fixture does not deliver native events, but must allow real cleanup.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'ui-review' }, currentWebview: { label: 'ui-review' } },
    transformCallback(callback) { const id = ++sequence; callbacks.set(id, callback); return id },
    unregisterCallback(id) { callbacks.delete(id) },
    convertFileSrc(path) { return '/fixture-assets/' + encodeURIComponent(path) },
    async invoke(method, params = {}) {
      calls.push({ method, params })
      if (method === 'plugin:store|load') return 1
      if (method === 'plugin:store|get') return [stored.get(params.key), stored.has(params.key)]
      if (method === 'plugin:store|set') { stored.set(params.key, params.value); return null }
      if (method === 'plugin:app|version') return '6.905.6'
      if (method === 'plugin:event|listen') return ++sequence
      if (method === 'theme_list') return [{ id: 'default', name: 'Default' }]
      if (['get_plugin_manifests', 'sotvault_records', 'notemd_mirror_metas'].includes(method)) return []
      if (method === 'sotvault_vault_root') return '/fixture-vault'
      if (method === 'notemd_vault_settings_get' || method === 'notemd_vault_settings_set') return {}
      if (method === 'vault_status') return { state: 'not_configured', configured: false, last_sync: null, error_message: null, has_conflicts: false }
      // Match src/lib/search/api.ts: indexState is non-null even when no
      // rebuild is running; only progress is nullable. Search tabs subscribe
      // and unsubscribe as navigation changes, just as in the real host.
      if (method === 'notemd_search_index_state') return { state: 'ready', error: null }
      if (method === 'notemd_search_stats') return {
        files: 128, blocks: 900, dbBytes: 4096, builtAt: '2026-09-05T09:00:00Z', tokenizerId: 'jieba-v1',
        skippedLarge: [{ path: 'sources/large-reference.md', sizeBytes: 20_000_000 }],
        originCounts: { human: 40, derived: 70, source: 18, unlabeled: 0 }, typeCounts: { Answer: 12 },
        attentionFiles: 24, attentionAsOf: '2026-09-05',
      }
      if (method === 'notemd_search_progress') return null
      if (method === 'notemd_search_rebuild' || method === 'notemd_search_reopen') return null
      if (method === 'notemd_search_glob_matches') return 18
      if (method === 'cli_install_status') return { installed: false, path: null }
      if (method === 'cli_install_candidates') return ['/fixture/bin']
      if (method === 'notemd_agents_search_section_missing') return false
      if (method === 'plugin_market_installed') return market.slice(0, 3).map((item) => ({ ...item, version: '9.0.0', enabled: true, capabilities: ['vault.read'] }))
      if (method === 'plugin_market_index') return { plugins: market }
      if (method === 'plugin_market_preview') return { ...market[3], capabilities: ['vault.read', 'vault.write'] }
      return null
    },
  }
}
