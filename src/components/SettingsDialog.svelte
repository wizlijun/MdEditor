<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { onMount } from 'svelte'
  import { ask, open as openFilePicker } from '@tauri-apps/plugin-dialog'
  import { settings, saveSettings, getPluginScopedAll, setPluginScopedValue, pluginScopedVersion } from '../lib/settings.svelte'
  import { i18n, setLocale, availableLocales, t, type Locale } from '../lib/i18n/store.svelte'
  import type { Messages } from '../lib/i18n/en'
  import { pluginTabLabel, pluginFieldLabel } from '../lib/plugins/plugin-i18n'
  import {
    updater as updaterState, runCheck as updaterRunCheck, setCheckOnStartup,
    downloadAndInstall as updaterDownloadAndInstall, restartApp as updaterRestart,
    hasUpdateForSettings,
  } from '../lib/updater.svelte'
  import { themes, loadThemes, reloadThemes } from '../lib/themes.svelte'
  import { pendingThemeImport } from '../lib/theme-import-bus.svelte'
  import ThemeImportDialog from './ThemeImportDialog.svelte'
  import { platform } from '../lib/platform.svelte'
  import { collectSettingsTabs, type SettingsTab } from '../lib/plugins/settings-registry'
  import { coreShareSettingsTab } from '../lib/share/settings-tab'
  import type { PluginManifest } from '../lib/plugins/types'
  import { isPluginActive } from '../lib/plugins/registry'
  import { outlineShortcuts, setShortcutOverride } from '../lib/outline/gate.svelte'
  import { outlineDirs, setOutlineDir } from '../lib/outline/dirs.svelte'
  import { inboxDir, setInboxDir } from '../lib/quick-note.svelte'
  import {
    vaultSettings, loadVaultSettings, saveSyncDir, DEFAULT_SYNC_DIR, saveLargeFileThreshold, DEFAULT_LARGE_FILE_THRESHOLD_MB,
    saveSearchExcludeDirs, saveSearchLargeFileThreshold, saveSearchSourceGlobs, saveSearchWeights, DEFAULT_SEARCH_WEIGHTS,
    type SearchWeights,
  } from '../lib/vault-settings.svelte'
  import { pushToast } from '../lib/toast.svelte'
  import { sotvaultStore } from '../lib/sotvault.svelte'
  import { indexStatus, estimateRebuildSeconds, elideMiddle, formatElapsedMs } from '../lib/search/index-status.svelte'
  import { searchApi, type SearchProgress } from '../lib/search/api'
  import { suggestGlobs } from '../lib/search/glob-suggest'
  import { searchStore } from '../lib/search/store.svelte'
  import { setActiveView, setSideVisible } from '../lib/side-panel/registry.svelte'
  import {
    DEFAULT_SHORTCUTS, resolveShortcuts, displayShortcut, eventToShortcut, findConflict,
    type OutlineCommandId,
  } from '../lib/outline/shortcuts'
  import VaultSettingsTab from './VaultSettingsTab.svelte'
  import { consumePendingSettingsTab } from '../lib/ui-state.svelte'
  import type { AgentOption } from '../lib/agent-picker/types'
  import {
    loadSearchAgentOptions,
    supportsSearchPlanner,
    supportsSearchTask,
    VAULT_RESEARCH_TASK,
  } from '../lib/smart-search/agent'

  let { open = $bindable(false) }: { open: boolean } = $props()

  let isIOSPlatform = $state(false)
  $effect(() => {
    platform().then((p) => { isIOSPlatform = p === 'ios' })
  })

  let pluginTabs = $state<SettingsTab[]>([])
  let selectedTab = $state<'core' | string>('core')
  let pluginValues = $state<Record<string, Record<string, unknown>>>({})
  let smartLookupAgents = $state<AgentOption[]>([])
  let smartLookupPlannerAgents = $derived(smartLookupAgents.filter(supportsSearchPlanner))
  let smartLookupHandoffAgents = $derived(
    smartLookupAgents.filter((agent) => supportsSearchTask(agent, VAULT_RESEARCH_TASK)),
  )

  // Callers that want the dialog to land on a specific tab (e.g. the search
  // panel's gear button) go through `openSettings(tab)`, which stashes the
  // request in `uiState.pendingSettingsTab` rather than writing `selectedTab`
  // directly — this module doesn't otherwise know about ui-state.svelte, and
  // routing through a pending flag means a repeat request for the same tab
  // (dialog already open, already on 'search') still re-fires because the
  // flag itself changed, not just its value.
  //
  // Deliberately NOT gated on `open`: this component is mounted once for the
  // app's lifetime (only its internal `{#if open}` block toggles), so this
  // effect runs on every change to `pendingSettingsTab` whether the dialog
  // is showing or not. `consumePendingSettingsTab()` (see ui-state.svelte.ts
  // for why) clears the flag unconditionally, so it can never survive to
  // redirect some later, unrelated open — e.g. a plain Preferences-menu
  // click landing on 'search' because the gear button set the flag earlier
  // and something skipped consuming it. Assigning `selectedTab` while closed
  // is harmless; it just means the dialog is already on the right tab by
  // the time it's shown.
  //
  // The flag is consumed unconditionally (before any decision below), but a
  // request for a tab this platform doesn't render is dropped rather than
  // honoured: iOS has no search commands and no `open_search_logs_window`,
  // so the tab button is `!isIOSPlatform`-gated in the strip and landing on
  // it via this path would show a page nothing can populate.
  $effect(() => {
    const tab = consumePendingSettingsTab()
    if (!tab) return
    if (tab === 'search' && isIOSPlatform) return
    selectedTab = tab
  })

  // Vault-scoped sync folder (stored in {vault}/.notemd/settings.json). Loaded
  // whenever the dialog opens; edited into a draft, then saved on demand.
  let syncDirDraft = $state('')
  let syncDirBusy = $state(false)
  let thresholdDraft = $state(DEFAULT_LARGE_FILE_THRESHOLD_MB)
  let thresholdBusy = $state(false)
  let searchExcludeDirsDraft = $state('')
  let searchExcludeDirsBusy = $state(false)
  // Search & Index tab: index-skip threshold. Loaded from
  // `vaultSettings.searchLargeFileThresholdMb`, which is already the
  // *effective* value (explicit save, else the git gate, else the built-in
  // default — see that field's doc comment in vault-settings.svelte.ts).
  let searchThresholdDraft = $state(DEFAULT_LARGE_FILE_THRESHOLD_MB)
  let searchThresholdBusy = $state(false)

  // Raw source glob patterns (task C-T8/C-T11, design spec §4.1/§7.1).
  // Loaded from `vaultSettings.searchSourceGlobs` (null = unconfigured →
  // empty draft, an empty saved list is a different, explicit state) and
  // edited as plain strings; validated only at save time (see
  // `onSaveSourceGlobs`).
  //
  // Review round 1, Important 2/Minor 4: `count` travels WITH the row
  // object, not in a separate index-keyed `Set`/array — the earlier version
  // kept a `Set<number>` of "zero match" row indices alongside a plain
  // `string[]` of patterns, which went stale the moment a row was removed
  // (every index after the removed one shifted, but the `Set` didn't) or
  // edited (the old count/warning kept describing text that was no longer
  // on screen). Folding `count` into the row itself means removal takes the
  // stale count with it for free, and every edit path below explicitly
  // resets `count` to `null` so a warning can never outlive the pattern it
  // was measured against. `null` = not fetched yet (or invalidated by an
  // edit); the count itself is what drives BOTH the always-visible "N
  // files" readout (Important 2 — the saved list showing no count at all
  // was the actual gap the ×0.3-scale incident scenario needs) and the
  // `count === 0` warning (spec §8), so there is only one source of truth
  // for both.
  //
  // Review round 2 nit: `'error'` is a THIRD state, distinct from `null`
  // ("not fetched yet" — still shows "…") — without it, a `globMatches`
  // failure (offline, a locked index, …) left the row showing "…" forever,
  // indistinguishable from an in-flight fetch that just hasn't resolved.
  type MatchCount = number | null | 'error'
  interface GlobRow { pattern: string; count: MatchCount }
  let globRows = $state<GlobRow[]>([])
  let globSaveBusy = $state(false)
  // §8: a blank row is rejected outright, naming the row.
  let globPatternBlankError = $state<string | null>(null)

  // "Generate from a sample path" (design spec §7.1) — `suggestGlobs` is
  // pure/local; each candidate's match count is fetched independently from
  // the real vault via `searchApi.globMatches` (see that function's doc
  // comment for why one call per candidate, not a batch).
  let globSampleDraft = $state('')
  let globCandidates = $state<{ pattern: string; count: MatchCount }[]>([])
  // The candidate list is presented narrow → wide by SCOPE, never re-sorted
  // by count (a reviewer proved the ladder can invert in exactly the mixed-
  // media folder this feature targets — see `glob-suggest.ts`'s doc
  // comment). Default-selecting `candidates[0]` is what makes the narrowest
  // — the file's own directory, the most *intentional* scope — the default,
  // regardless of which candidate turns out to match fewer files.
  let globCandidateSelected = $state<string | null>(null)

  // Per-tier ranking weights (task C-T7/C-T11, design spec §3.1/§7.3).
  // Loaded from `vaultSettings.searchWeights`, which already falls back to
  // `DEFAULT_SEARCH_WEIGHTS` for any unset field.
  let weightsDraft = $state<SearchWeights>({ ...DEFAULT_SEARCH_WEIGHTS })
  let weightsBusy = $state(false)

  // `null` = not checked yet (or no vault / no AGENTS.md to check). Detection
  // is read-only (notemd_agents_search_section_missing); the write
  // (notemd_agents_append_search_section) only ever runs from onAddAgentsSearchSection,
  // after the user clicks the button below — never on load, never silently.
  let agentsSectionMissing = $state<boolean | null>(null)
  let agentsSectionBusy = $state(false)
  $effect(() => {
    if (!open) return
    void loadVaultSettings().then(() => {
      syncDirDraft = vaultSettings.syncDir
      thresholdDraft = vaultSettings.largeFileThresholdMb
      searchExcludeDirsDraft = vaultSettings.searchExcludeDirs.join('\n')
      searchThresholdDraft = vaultSettings.searchLargeFileThresholdMb
      globRows = (vaultSettings.searchSourceGlobs ?? []).map((p) => ({ pattern: p, count: null }))
      globPatternBlankError = null
      refreshGlobCounts()
      weightsDraft = { ...vaultSettings.searchWeights }
      if (!vaultSettings.vaultPath) { agentsSectionMissing = null; return }
      void invoke<boolean>('notemd_agents_search_section_missing')
        .then((missing) => { agentsSectionMissing = missing })
        .catch(() => { agentsSectionMissing = null })
    })
  })
  async function onAddAgentsSearchSection() {
    agentsSectionBusy = true
    try {
      await invoke('notemd_agents_append_search_section')
      agentsSectionMissing = false
      pushToast({ level: 'success', message: t('search.agentsAdded') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      agentsSectionBusy = false
    }
  }
  async function onSetOutlineDir(kind: 'wikipage' | 'dailynote', value: string) {
    try {
      await setOutlineDir(kind, value)
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    }
  }
  async function onSetInboxDir(value: string) {
    try {
      await setInboxDir(value)
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    }
  }
  async function onSaveSyncDir() {
    syncDirBusy = true
    try {
      await saveSyncDir(syncDirDraft)
      syncDirDraft = vaultSettings.syncDir
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      syncDirBusy = false
    }
  }
  async function onSaveThreshold() {
    const mb = Math.max(1, Math.floor(Number(thresholdDraft) || DEFAULT_LARGE_FILE_THRESHOLD_MB))
    thresholdBusy = true
    try {
      await saveLargeFileThreshold(mb)
      thresholdDraft = vaultSettings.largeFileThresholdMb
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      thresholdBusy = false
    }
  }
  async function onSaveSearchExcludeDirs() {
    const dirs = searchExcludeDirsDraft
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
    searchExcludeDirsBusy = true
    try {
      await saveSearchExcludeDirs(dirs)
      searchExcludeDirsDraft = vaultSettings.searchExcludeDirs.join('\n')
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      searchExcludeDirsBusy = false
    }
  }
  // The one-way door: saving here always writes an explicit value, even if
  // it happens to equal the git gate's current number — from that point on
  // `vaultSettings.searchLargeFileThresholdExplicit` is true and this stops
  // following `largeFileThresholdMb` for good (see the field's doc comment
  // in vault-settings.svelte.ts, and `search.index.thresholdHint` above the
  // input in the template).
  async function onSaveSearchThreshold() {
    const mb = Math.max(1, Math.floor(Number(searchThresholdDraft) || DEFAULT_LARGE_FILE_THRESHOLD_MB))
    searchThresholdBusy = true
    try {
      await saveSearchLargeFileThreshold(mb)
      searchThresholdDraft = vaultSettings.searchLargeFileThresholdMb
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      searchThresholdBusy = false
    }
  }

  // ---- Raw source glob patterns (task C-T8/C-T11, design spec §4.1/§7.1) ----

  // Shared by the saved-pattern list and the candidate ladder. `'error'`
  // (review round 2 nit) renders distinctly from `null` ("still loading")
  // so a `globMatches` failure doesn't look identical to an in-flight fetch
  // forever.
  function formatMatchCount(count: MatchCount): string {
    if (count === null) return '…'
    if (count === 'error') return t('search.index.globsCountUnavailable')
    return t('search.index.globsMatchCount', { n: count })
  }

  // The vault-relative pattern the backend seeds when `searchSourceGlobs` is
  // `null` (never configured) — mirrors `search::options::source_globs_from`
  // exactly (`<resolved syncDir>/**`; see that function's doc comment for
  // why it reads the vault's currently-resolved sync dir, not the literal
  // `"sync/**"`). Used ONLY for display (review round 1, Important 3): the
  // settings page must show what's actually in effect before the user
  // replaces it, not leave an unconfigured vault looking identical to an
  // intentionally-empty one.
  //
  // Known imprecision (review round 2 nit, not fixed): this reads
  // `vaultSettings.syncDir` VERBATIM, the raw stored value — the backend's
  // actual seed instead goes through `vault_settings::resolve_sync_dir_from`,
  // which re-validates it (`validate_rel_dir`: rejects empty/absolute/`..`-
  // escaping values) and falls back to `DEFAULT_SYNC_DIR` on failure. A
  // hand-edited, invalid `syncDir` in `.notemd/settings.json` would make
  // this display a pattern that ISN'T the real effective one. Not fixed
  // here: re-implementing `validate_rel_dir` in TS would be exactly the
  // kind of cross-language duplication this codebase avoids elsewhere
  // (`search::options`'s own doc comment: "GUI 与 CLI 是两个进程... 逐字段
  // 一致") — the class of bug (drift between two implementations of the same
  // rule) is the same one C-T7's `weights_for_vault`/CLI contract test
  // exists to prevent for weights. A proper fix needs either a dedicated
  // backend command that returns the truly-resolved value, or moving this
  // validation somewhere both sides can share — out of scope for a nit.
  // `vaultSettings.syncDir` already carries this same imprecision anywhere
  // else it's displayed on this tab (e.g. the sync-dir field above), so this
  // is not a NEW gap, just one more place inheriting an existing one.
  let effectiveDefaultGlob = $derived(`${vaultSettings.syncDir}/**`)

  // Design spec §7.1: counts come from the real vault, never the index (see
  // `searchApi.globMatches`'s doc comment) — one call per pattern, fired in
  // parallel. Closes over each `row` OBJECT rather than an index, so a row
  // added/removed/reordered while a fetch is in flight can't misattribute
  // its result to the wrong row (the `Set<number>` this replaced could).
  // Guards against a since-edited pattern overwriting a stale in-flight
  // result with `row.pattern.trim() === p`.
  function refreshGlobCounts() {
    for (const row of globRows) {
      const p = row.pattern.trim()
      if (!p) continue
      void searchApi.globMatches([p]).then((n) => {
        if (row.pattern.trim() === p) row.count = n
      }).catch(() => {
        if (row.pattern.trim() === p) row.count = 'error'
      })
    }
  }

  function onAddGlobRow() {
    globRows = [...globRows, { pattern: '', count: null }]
  }
  function onRemoveGlobRow(i: number) {
    globRows = globRows.filter((_, idx) => idx !== i)
  }
  // Review round 1, Minor 4: editing a row's text must invalidate ITS OWN
  // `count` immediately — the old index-keyed `Set` left a stale "matches 0
  // files" (or a stale non-zero count) attached to text the user had
  // already changed. Folding `count` into the row object (see `GlobRow`'s
  // declaration) makes this a one-line reset instead of a second data
  // structure to keep in sync.
  function onGlobRowInput(i: number, value: string) {
    globRows[i].pattern = value
    globRows[i].count = null
  }
  function onGlobRowBlur(i: number) {
    const row = globRows[i]
    const p = row?.pattern.trim()
    if (!p) return
    void searchApi.globMatches([p]).then((n) => {
      if (row.pattern.trim() === p) row.count = n
    }).catch(() => {
      if (row.pattern.trim() === p) row.count = 'error'
    })
  }

  // §8, two DIFFERENT error handlings, plus review round 1's Important 3:
  //  - a blank row is rejected outright, naming which row, and the save
  //    never reaches the backend — `searchidx::globs::parse` tolerates a
  //    blank/unparseable entry (drops it silently), but that tolerance is a
  //    *consumer* obligation, not license for the UI to silently drop a row
  //    the user just typed and believes was saved.
  //  - a pattern matching 0 files is ALLOWED to save; flagged per-row
  //    afterward instead. This is the only safety net for a vault directory
  //    whose case was changed (§4.1 literal case-sensitive matching) — every
  //    pattern stays syntactically valid while matching nothing.
  //  - saving an EMPTY list needs its own confirmation: for every vault
  //    that has never configured this setting, the effective pattern is
  //    `effectiveDefaultGlob` (see above), never nothing — a click here
  //    replaces that implicit default with an explicit empty list, which
  //    demotes every file that default was tagging `Source` down to
  //    `Unlabeled` (×0.3) the moment the triggered rebuild finishes. That is
  //    real enough (and different enough from every other save on this
  //    page) to earn its own `ask()` rather than the ordinary Save button.
  async function onSaveSourceGlobs() {
    globPatternBlankError = null
    const blankIndex = globRows.findIndex((r) => r.pattern.trim() === '')
    if (blankIndex !== -1) {
      globPatternBlankError = t('search.index.globsBlankError', { row: String(blankIndex + 1) })
      return
    }
    const patterns = globRows.map((r) => r.pattern.trim())
    if (patterns.length === 0) {
      const ok = await ask(
        t('search.index.globsEmptySaveConfirmBody', { default: effectiveDefaultGlob }),
        { title: t('search.index.globsEmptySaveConfirmTitle'), kind: 'warning', okLabel: t('vaultSync.save'), cancelLabel: t('common.cancel') },
      )
      if (!ok) return
    }
    globSaveBusy = true
    try {
      await saveSearchSourceGlobs(patterns)
      globRows = (vaultSettings.searchSourceGlobs ?? []).map((p) => ({ pattern: p, count: null }))
      refreshGlobCounts()
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      globSaveBusy = false
    }
  }

  // "Generate from a sample path" — narrow-to-wide candidates from the pure
  // `suggestGlobs`, each candidate's match count fetched independently from
  // the real vault. Deliberately NOT sorted by count and NOT auto-selecting
  // whichever matches fewest — see `globCandidateSelected`'s declaration.
  function onGenerateGlobCandidates() {
    const sample = globSampleDraft.trim()
    const candidates = suggestGlobs(sample)
    globCandidates = candidates.map((c) => ({ pattern: c.pattern, count: null }))
    globCandidateSelected = candidates[0]?.pattern ?? null
    for (const c of candidates) {
      void searchApi.globMatches([c.pattern]).then((n) => {
        globCandidates = globCandidates.map((x) => (x.pattern === c.pattern ? { ...x, count: n } : x))
      }).catch(() => {
        globCandidates = globCandidates.map((x) => (x.pattern === c.pattern ? { ...x, count: 'error' as const } : x))
      })
    }
  }
  function onUseSelectedCandidate() {
    if (!globCandidateSelected) return
    if (!globRows.some((r) => r.pattern === globCandidateSelected)) {
      // Reuses the candidate's already-fetched count instead of re-querying
      // — `globMatches` was already called for every candidate above.
      const candidate = globCandidates.find((c) => c.pattern === globCandidateSelected)
      globRows = [...globRows, { pattern: globCandidateSelected, count: candidate?.count ?? null }]
    }
    globCandidates = []
    globCandidateSelected = null
    globSampleDraft = ''
  }

  // ---- Per-tier ranking weights (task C-T7/C-T11, design spec §3.1/§7.3) ----

  // The tier fields this page has inputs for. `SearchWeights` also carries
  // `attention` (the attention-boost `k`), which has NO input here — it is
  // only round-tripped, see `onResetWeightsDraft` and `vault-settings`'s
  // `SearchWeights` doc comment. `Exclude` rather than `keyof` so adding a
  // sixth backend field can never silently start being validated/labelled as
  // if it were a tier.
  type WeightField = Exclude<keyof SearchWeights, 'attention'>
  const WEIGHT_FIELDS: WeightField[] = ['human', 'derived', 'source', 'unlabeled']
  function weightFieldLabel(f: WeightField): string {
    if (f === 'human') return t('search.group.human')
    if (f === 'derived') return t('search.index.tiersDerivedLabel')
    if (f === 'source') return t('search.group.source')
    return t('search.group.unlabeled')
  }
  // Mirrors `vault_settings::validate_search_weights`'s bounds exactly
  // (finite, > 0, <= 5.0) — review round 1, Minor 6: Svelte's `bind:value`
  // on a `type="number"` input writes `null` for an empty/unparseable
  // field, silently defeating the `SearchWeights` TS type. Sending that
  // `null` through used to reach the backend, which stores `search_weights`
  // as ONE WHOLESALE STRUCT (`vault_settings.rs`'s `merge` does
  // `out.search_weights = Some(w)`, not a per-field merge) — so a cleared
  // `human` field didn't just fail to update `human`, it silently replaced
  // the entire stored weights (including any other previously-customized
  // field) with one where `human` reads back as the 1.25 default, while
  // this component still showed a plain "Saved" success toast. Catching it
  // HERE, before the request is ever sent, is what makes spec §8's
  // "reject, keep the previous value" promise true for this failure mode:
  // the backend is never even asked, so whatever was stored stays stored.
  function invalidWeightField(w: SearchWeights): WeightField | null {
    for (const f of WEIGHT_FIELDS) {
      const v = w[f]
      if (typeof v !== 'number' || !Number.isFinite(v) || v <= 0 || v > 5) return f
    }
    return null
  }
  let weightsError = $state<string | null>(null)

  // Fills the draft only — does NOT save. Consistent with every other
  // draft-then-Save control on this tab; the user still has to click Save
  // for "restore defaults" to actually take effect.
  // `attention` is deliberately carried over from the current draft instead
  // of being reset: this button restores the defaults of the four tier
  // weights it sits under, and `attention` has no control on this page at
  // all. Resetting a value the user can only have set by hand — and whose
  // interesting value is `0`, "turn attention weighting off" — from a button
  // that never mentions it would be the same silent wipe as final review I-2,
  // just via a different path.
  function onResetWeightsDraft() {
    weightsDraft = { ...DEFAULT_SEARCH_WEIGHTS, attention: weightsDraft.attention }
    weightsError = null
  }
  async function onSaveWeights() {
    weightsError = null
    const badField = invalidWeightField(weightsDraft)
    if (badField) {
      weightsError = t('search.weights.invalidError', { label: weightFieldLabel(badField) })
      return
    }
    weightsBusy = true
    try {
      await saveSearchWeights({ ...weightsDraft })
      weightsDraft = { ...vaultSettings.searchWeights }
      pushToast({ level: 'success', message: t('vaultSync.saved') })
    } catch (e) {
      // Backend rejection keeps the previously-stored value (design spec
      // §8) — reload the draft from it so the UI doesn't keep showing the
      // rejected numbers as if they were live. The client-side check above
      // is expected to catch the common case (a cleared input); this stays
      // as a backstop for whatever it can't (e.g. a stale draft racing a
      // concurrent write from another window).
      weightsDraft = { ...vaultSettings.searchWeights }
      pushToast({ level: 'error', message: t('vaultSync.saveFailed', { error: String(e) }), detail: String(e) })
    } finally {
      weightsBusy = false
    }
  }

  // Design spec §7.4: the "Unlabeled" tier row is the designed exit from the
  // ×0.3 demotion. Surfaces the search panel (registering/showing it if it
  // isn't already), runs `origin:unlabeled`, and closes this dialog so the
  // results are actually visible — same `open = false` idiom the Done
  // button and overlay-click handler below use, not `closeSettings()` (see
  // that function's doc comment on why the hot close path writes the
  // bindable prop directly).
  async function onUnlabeledTierClick() {
    await setActiveView('left', 'vault-search')
    await setSideVisible('left', true)
    open = false
    await searchStore.run('origin:unlabeled', { deep: true })
  }

  type CliStatus = { installed: boolean; path: string | null; target_valid: boolean }
  let cliStatus = $state<CliStatus | null>(null)
  let cliBusy = $state(false)
  let cliError = $state<string | null>(null)

  let updaterBusy = $state(false)
  let updaterMessage = $state<string | null>(null)

  async function handleCheckUpdate() {
    updaterBusy = true
    updaterMessage = null
    try {
      await updaterRunCheck({ forceFresh: true })
      if (updaterState.state === 'uptodate') {
        updaterMessage = t('settings.update.upToDate')
      } else if (updaterState.state === 'available') {
        updaterMessage = t('settings.update.foundNew', { version: updaterState.latestVersion ?? '' })
      }
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    } finally {
      updaterBusy = false
    }
  }

  async function handleUpdateNow() {
    updaterBusy = true
    updaterMessage = null
    try {
      await updaterDownloadAndInstall()
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    } finally {
      updaterBusy = false
    }
  }

  async function handleRestart() {
    try {
      await updaterRestart()
    } catch (e) {
      updaterMessage = e instanceof Error ? e.message : String(e)
    }
  }

  async function onCheckOnStartupToggle(e: Event) {
    await setCheckOnStartup((e.currentTarget as HTMLInputElement).checked)
  }

  function formatLastChecked(iso: string | null): string {
    if (!iso) return t('time.never')
    const ms = Date.parse(iso)
    if (!Number.isFinite(ms)) return iso
    const d = new Date(ms)
    return d.toLocaleString()
  }

  async function refreshCliStatus() {
    try {
      cliStatus = await invoke<CliStatus>('cli_install_status')
    } catch (e) {
      console.warn('[SettingsDialog] cli_install_status:', e)
      cliStatus = { installed: false, path: null, target_valid: false }
    }
  }

  async function handleCliInstall() {
    cliBusy = true
    cliError = null
    try {
      const candidates = await invoke<string[]>('cli_install_candidates')
      const { ask } = await import('@tauri-apps/plugin-dialog')
      for (const dir of candidates) {
        const ok = await ask(`Install 'notemd' into ${dir}?`, {
          title: "Install 'notemd' Command",
          kind: 'info',
        })
        if (ok) {
          await invoke('cli_install', { dir })
          break
        }
      }
    } catch (e) {
      cliError = e instanceof Error ? e.message : String(e)
    } finally {
      cliBusy = false
      await refreshCliStatus()
    }
  }

  async function handleCliUninstall() {
    if (!cliStatus?.installed || !cliStatus.path) return
    cliBusy = true
    cliError = null
    try {
      const dir = cliStatus.path.replace(/\/(?:notemd|mdedit)$/, '')
      await invoke('cli_uninstall', { dir })
    } catch (e) {
      cliError = e instanceof Error ? e.message : String(e)
    } finally {
      cliBusy = false
      await refreshCliStatus()
    }
  }

  onMount(async () => {
    try {
      const manifests = await invoke<PluginManifest[]>('get_plugin_manifests')
      // Defensive guard: no shipped manifest contributes a share settings tab
      // anymore (share is core), but filter regardless so a stray external
      // 'share' plugin can't duplicate the core Share tab.
      pluginTabs = [coreShareSettingsTab(), ...collectSettingsTabs(manifests.filter((m) => m.id !== 'share'))]
    } catch (e) {
      console.warn('[SettingsDialog] manifest load:', e)
      pluginTabs = [coreShareSettingsTab()]
    }
    try {
      smartLookupAgents = await loadSearchAgentOptions()
      let changed = false
      if (settings.smartLookup.planner.provider !== 'auto'
        && !smartLookupPlannerAgents.some((agent) => agent.id === settings.smartLookup.planner.provider)) {
        settings.smartLookup.planner.provider = 'auto'
        changed = true
      }
      if (settings.smartLookup.handoff.defaultProvider !== 'ask'
        && !smartLookupHandoffAgents.some((agent) => agent.id === settings.smartLookup.handoff.defaultProvider)) {
        settings.smartLookup.handoff.defaultProvider = 'ask'
        changed = true
      }
      if (changed) void saveSettings()
    } catch {
      smartLookupAgents = []
    }
    void refreshCliStatus()
  })

  $effect(() => {
    void pluginScopedVersion.value
    if (!open || pluginTabs.length === 0) return
    for (const tab of pluginTabs) {
      const all = getPluginScopedAll(tab.pluginId)
      const stripped: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(all)) {
        stripped[k.slice(tab.pluginId.length + 1)] = v
      }
      pluginValues[tab.pluginId] = stripped
    }
  })

  // Everything the Search & Index tab needs on entry, in ONE place keyed on
  // "the tab is showing" — deliberately not split between here and the tab
  // strip's `onclick`. That split is what made the gear button in
  // `SearchPanel.svelte` (which routes through `openSettings('search')` →
  // `pendingSettingsTab` → `selectedTab`, never touching the tab button)
  // render a fully built index as "— / — / Last built: Never / no files are
  // currently skipped" — an affirmative false claim on the very panel built
  // to explain why a file is missing from search. Any future entry point
  // that lands on this tab gets the data load for free.
  //
  //  - `refresh()`: initial snapshot (stats + a poll of `progress()` so a
  //    page opened mid-rebuild isn't blank until the next event).
  //  - `searchThresholdDraft`: re-derived on every entry, not only when the
  //    dialog first opens — otherwise editing the git gate on the Vault tab
  //    and switching here (without closing the dialog) shows a stale
  //    effective value that contradicts the hint text under the input.
  //    Reading `vaultSettings.searchLargeFileThresholdMb` here also makes
  //    that the effect's dependency, so the draft follows a live change to
  //    the gate while the tab is already open.
  //  - the returned unsubscribe: live `search://progress` /
  //    `search://index-updated` while the tab is showing, torn down whenever
  //    `selectedTab` moves away or the dialog closes (the whole `{#if open}`
  //    block unmounts) — same $effect-return-cleanup idiom as
  //    `SearchPanel.svelte`'s own event subscription.
  $effect(() => {
    if (selectedTab !== 'search') return
    void indexStatus.refresh()
    searchThresholdDraft = vaultSettings.searchLargeFileThresholdMb
    return indexStatus.subscribe()
  })

  function formatBytes(n: number): string {
    if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
    if (n >= 1024) return `${Math.round(n / 1024)} KB`
    return `${n} B`
  }

  const PHASE_LABEL_KEYS: Record<string, keyof Messages> = {
    walking: 'search.index.phase.walking',
    indexing: 'search.index.phase.indexing',
    removing: 'search.index.phase.removing',
  }
  // `phase` is typed as a closed union in `SearchProgress`, but the payload
  // crosses an IPC boundary from Rust — falling back to the raw string for
  // an unrecognized value is safer than throwing away the whole progress
  // block over a label lookup miss.
  function phaseLabel(phase: SearchProgress['phase']): string {
    const key = PHASE_LABEL_KEYS[phase]
    return key ? t(key) : phase
  }

  function progressPercent(p: SearchProgress): number {
    if (p.total <= 0) return 0
    return Math.min(100, Math.round((p.done / p.total) * 100))
  }

  // Confirmation text must say plainly: full rebuild, search unavailable
  // meanwhile, roughly how long, and — the part a user would otherwise
  // reasonably fear — that no notes are lost (the index is a disposable
  // derivative of the markdown files, not the files themselves). Uses the
  // codebase's existing `confirm()` dialog pattern (see e.g.
  // `cmdMdblockReset` in lib/mdblock/commands.ts) rather than a hand-rolled
  // modal.
  async function onRebuildIndex() {
    await indexStatus.requestRebuild(async () => {
      const { confirm } = await import('@tauri-apps/plugin-dialog')
      const files = indexStatus.stats?.files
      const body = files != null
        ? t('search.index.rebuildConfirmBody', { files, seconds: estimateRebuildSeconds(files) })
        : t('search.index.rebuildConfirmBodyUnknown')
      return await confirm(body, { title: t('search.index.rebuildConfirmTitle'), kind: 'warning' })
    })
  }

  // Design spec §5/§6.1/§7: jump to the log window pre-filtered to the
  // `search` category. `open_search_logs_window` is the thin frontend-facing
  // wrapper around the host's existing `open_logs_window(app, filter)`
  // (see `src-tauri/src/lib.rs`) — a failure (window can't be built) must
  // not block the rest of the settings page, so it's a toast, not a thrown
  // error left for the caller.
  async function onViewSearchLogs() {
    try {
      await invoke('open_search_logs_window')
    } catch (e) {
      pushToast({ level: 'error', message: t('search.index.viewLogsFailed', { error: String(e) }), detail: String(e) })
    }
  }

  async function savePluginField(pluginId: string, key: string, value: unknown) {
    pluginValues[pluginId] = { ...(pluginValues[pluginId] ?? {}), [key]: value }
    await setPluginScopedValue(pluginId, key, value)
  }

  async function savePluginNumberField(pluginId: string, key: string, input: HTMLInputElement) {
    const value = input.valueAsNumber
    if (!Number.isFinite(value) || !input.checkValidity()) return
    await savePluginField(pluginId, key, value)
  }

  // The 22 categories cover every extension our editor supports as a document type.
  // These must match the `fileAssociations` in src-tauri/tauri.conf.json — that's
  // what tells macOS that note.md is a legitimate handler for these UTIs in the first place.
  const FILE_GROUPS: { label: string; exts: string[] }[] = [
    { label: 'Markdown',      exts: ['md', 'markdown', 'mdown', 'mkd'] },
    // NOTE: intentionally NOT registering html/htm here. Setting note.md as the
    // default handler for public.html makes macOS treat it as a web-browser
    // candidate and lets it hijack the default browser. Passive "Open With"
    // support (Info.plist) stays, but we never claim the default role.
    // 只剩 Markdown + 纯文本。此前还登记了 20 类代码/配置扩展名(py/rs/go/
    // sql/…),让 note.md 挤进每个源码文件的「打开方式」——对代码文件是打扰
    // 不是价值。要用 note.md 开这些文件,拖进窗口或「打开方式 → 其他」仍可以。
    { label: 'Plain text',    exts: ['txt', 'text'] },
  ]
  const ALL_EXTS = FILE_GROUPS.flatMap((g) => g.exts)

  type ExtResult = {
    ext: string
    uti: string | null
    ok: boolean
    error: string | null
  }

  let busy = $state(false)
  let resultText = $state<string | null>(null)
  let resultDetails = $state<ExtResult[] | null>(null)

  async function handleSetDefault() {
    const ok = await ask(
      `This will register note.md as the macOS default application for ${ALL_EXTS.length} ` +
        `file extensions across ${FILE_GROUPS.length} categories.\n\n` +
        `From then on, double-clicking any of these file types in Finder will open them in note.md instead of your current default editor.\n\n` +
        `Categories: ${FILE_GROUPS.map((g) => g.label).join(', ')}.\n\n` +
        `You can revert this for any single type later: in Finder, select a file → Get Info → "Open with" → choose another app → click "Change All…".\n\n` +
        `Continue?`,
      {
        title: 'Set note.md as default for text & code files',
        kind: 'warning',
        okLabel: 'Set as default',
        cancelLabel: 'Cancel',
      },
    )
    if (!ok) return
    busy = true
    resultText = null
    resultDetails = null
    try {
      const results = await invoke<ExtResult[]>('set_default_app_for_extensions', {
        exts: ALL_EXTS,
      })
      resultDetails = results
      const successes = results.filter((r) => r.ok).length
      const failures = results.filter((r) => !r.ok)
      if (failures.length === 0) {
        resultText = t('settings.defaultApp.resultOk', { count: successes })
      } else {
        resultText = t('settings.defaultApp.resultPartial', {
          ok: successes,
          total: results.length,
          failed: failures.map((f) => `.${f.ext}`).join(', '),
        })
      }
    } catch (e) {
      resultText = t('settings.defaultApp.resultError', { error: e instanceof Error ? e.message : String(e) })
    } finally {
      busy = false
    }
  }

  async function onToggle(e: Event) {
    settings.autoSave = (e.currentTarget as HTMLInputElement).checked
    await saveSettings()
  }

  let importReport = $state<unknown | null>(null)
  let importBusy = $state(false)

  // ── git proxy ────────────────────────────────────────────────────────────
  // Machine-local, so it lives in shared.json rather than the vault's own
  // settings (which sync). Loaded lazily the first time the dialog opens.
  let gitProxyInput = $state('')
  let gitProxyBusy = $state(false)
  let gitProxyError = $state<string | null>(null)
  let gitProxySaved = $state(false)
  let gitProxyLoaded = false

  $effect(() => {
    if (!open || gitProxyLoaded || isIOSPlatform) return
    gitProxyLoaded = true
    void (async () => {
      const { getGitProxy } = await import('../lib/shared-config')
      gitProxyInput = await getGitProxy().catch(() => '')
    })()
  })

  async function onSaveGitProxy() {
    gitProxyBusy = true
    gitProxyError = null
    gitProxySaved = false
    try {
      const { setGitProxy } = await import('../lib/shared-config')
      // The host returns the normalized value, so the field shows exactly what
      // was stored (trimmed) rather than what was typed.
      gitProxyInput = await setGitProxy(gitProxyInput)
      gitProxySaved = true
    } catch (e) {
      // Surfaced verbatim: the host writes these to be read by a human
      // ("unsupported proxy scheme 'ftp' — use http, https, socks5 or socks5h").
      gitProxyError = typeof e === 'string' ? e : String(e)
    } finally {
      gitProxyBusy = false
    }
  }

  // Mirror the App.svelte drag-drop bus: when a .zip is dropped, App stuffs
  // the prepared ImportReport into pendingThemeImport.report; we surface it
  // here so the ThemeImportDialog modal renders.
  $effect(() => {
    if (pendingThemeImport.report) {
      importReport = pendingThemeImport.report
      pendingThemeImport.report = null
    }
  })

  async function onLightThemeChange(e: Event) {
    settings.theme.light = (e.currentTarget as HTMLSelectElement).value
    await saveSettings()
  }
  async function onDarkThemeChange(e: Event) {
    settings.theme.dark = (e.currentTarget as HTMLSelectElement).value
    await saveSettings()
  }
  async function onFollowSystemToggle(e: Event) {
    settings.theme.followSystem = !(e.currentTarget as HTMLInputElement).checked
    // Note: the *checkbox label* says "Always use light theme", so
    // checked means !followSystem.
    await saveSettings()
  }

  async function handleImportTheme() {
    const selection = await openFilePicker({
      multiple: false,
      directory: false,
      filters: [{ name: 'Typora theme zip', extensions: ['zip'] }],
    })
    if (!selection || Array.isArray(selection)) return
    importBusy = true
    try {
      importReport = await invoke('theme_import', { zipPath: selection })
    } catch (e) {
      console.warn('[Settings] theme_import:', e)
      importReport = { themes: [], asset_dirs: [], errors: [{ file: '?', message: String(e) }], staging_dir: '' }
    } finally {
      importBusy = false
    }
  }

  async function handleRevealThemes() {
    try { await invoke('theme_reveal') }
    catch (e) { console.warn('[Settings] theme_reveal:', e) }
  }

  async function handleReloadThemes() {
    await reloadThemes()
  }

  async function handleRestoreBuiltins() {
    try { await invoke('theme_restore_builtins') }
    catch (e) { console.warn('[Settings] theme_restore_builtins:', e) }
    await reloadThemes()
  }

  $effect(() => { void loadThemes() })

  // Outline shortcut rebinding
  const isMac = typeof navigator !== 'undefined' && navigator.platform.includes('Mac')
  let recording = $state<OutlineCommandId | null>(null)
  let conflictMsg = $state('')
  let resolvedOutline = $derived(resolveShortcuts(outlineShortcuts.overrides))

  const OUTLINE_CMD_LABELS: Record<OutlineCommandId, keyof Messages> = {
    'outline.indent': 'outline.cmd.indent',
    'outline.outdent': 'outline.cmd.outdent',
    'outline.toggleCollapse': 'outline.cmd.toggleCollapse',
    'outline.moveUp': 'outline.cmd.moveUp',
    'outline.moveDown': 'outline.cmd.moveDown',
    'outline.bold': 'outline.cmd.bold',
    'outline.italic': 'outline.cmd.italic',
  }

  async function onRecordKey(e: KeyboardEvent, id: OutlineCommandId) {
    e.preventDefault(); e.stopPropagation()
    if (e.key === 'Escape') { recording = null; conflictMsg = ''; return }
    const sc = eventToShortcut(e)
    if (!sc) return
    const trial = resolveShortcuts({ ...outlineShortcuts.overrides, [id]: sc })
    const conflict = findConflict(trial, id)
    if (conflict) { conflictMsg = t('outline.shortcutConflict', { other: t(OUTLINE_CMD_LABELS[conflict]) }); return }
    conflictMsg = ''
    await setShortcutOverride(id, sc)
    recording = null
  }
</script>

{#if open}
  <div
    class="overlay"
    role="presentation"
    onclick={() => (open = false)}
    onkeydown={(e) => e.key === 'Escape' && (open = false)}
  >
    <div class="dialog" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
      <h2>{t('settings.title')}</h2>

      <nav class="tab-strip">
        <button class:active={selectedTab === 'core'} onclick={() => selectedTab = 'core'}>{t('settings.tab.core')}</button>
        <button class:active={selectedTab === 'block'} onclick={() => selectedTab = 'block'}>{t('settings.tab.block')}</button>
        {#if !isIOSPlatform}
          <button class:active={selectedTab === 'cli'} onclick={() => { selectedTab = 'cli'; void refreshCliStatus() }}>{t('settings.tab.cli')}</button>
          <button class:active={selectedTab === 'updates'} onclick={() => { selectedTab = 'updates' }}>{t('settings.tab.updates')}</button>
        {/if}
        {#if isIOSPlatform}
          <button class:active={selectedTab === 'vault'} onclick={() => selectedTab = 'vault'}>{t('settings.tab.vault')}</button>
        {/if}
        {#if !isIOSPlatform}
          <!-- iOS registers no search commands at all (see the `#[cfg(not(
               target_os = "ios"))]` gates in src-tauri) and
               `open_search_logs_window` doesn't exist there either, so this
               tab could only ever show a no-vault message plus a View-logs
               button that errors on click. Gated like CLI/Updates above.
               Data loading lives in the `selectedTab === 'search'` $effect,
               not here — see its comment for why. -->
          <button class:active={selectedTab === 'search'} onclick={() => selectedTab = 'search'}>
            {t('settings.tab.search')}
          </button>
        {/if}
        <button class:active={selectedTab === 'outline-notes'} onclick={() => selectedTab = 'outline-notes'}>{t('settings.tab.outline')}</button>
        {#each pluginTabs as ptab (ptab.pluginId)}
          <button class:active={selectedTab === ptab.pluginId} onclick={() => selectedTab = ptab.pluginId}>{pluginTabLabel(ptab.manifest, ptab.label)}</button>
        {/each}
      </nav>

      {#if selectedTab === 'core'}
        <section class="block">
          <label class="row">
            <span class="lbl">{t('settings.language')}</span>
            <select value={i18n.locale} onchange={(e) => setLocale((e.currentTarget as HTMLSelectElement).value as Locale)}>
              {#each availableLocales as loc (loc.code)}
                <option value={loc.code}>{loc.label}</option>
              {/each}
            </select>
          </label>
        </section>

        <section class="block">
          <h3>{t('settings.themes')}</h3>
          <label class="row">
            <span class="lbl">{t('settings.lightTheme')}</span>
            <select value={settings.theme.light} onchange={onLightThemeChange}>
              {#each themes.list as th (th.id)}
                <option value={th.id}>{th.name}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('settings.darkTheme')}</span>
            <select value={settings.theme.dark} onchange={onDarkThemeChange}>
              {#each themes.list as th (th.id)}
                <option value={th.id}>{th.name}</option>
              {/each}
            </select>
          </label>
          <label class="row" style="margin-top: 6px;">
            <input
              type="checkbox"
              checked={!settings.theme.followSystem}
              onchange={onFollowSystemToggle}
            />
            {t('settings.alwaysLight')}
          </label>
          <div class="row" style="gap: 8px; flex-wrap: wrap; margin-top: 8px;">
            <button onclick={handleImportTheme} disabled={importBusy}>
              {importBusy ? t('themeImport.importing') : t('settings.importTypora')}
            </button>
            <button onclick={handleRevealThemes}>{t('settings.revealThemes')}</button>
            <button onclick={handleReloadThemes}>{t('settings.reloadThemes')}</button>
            <button onclick={handleRestoreBuiltins}>{t('settings.restoreBuiltins')}</button>
          </div>
          {#if themes.error}
            <p class="desc" style="color: tomato;">{t('settings.themesLoadFailed', { error: themes.error })}</p>
          {/if}
        </section>

        {#if importReport}
          <ThemeImportDialog
            report={importReport as never}
            onClose={() => { importReport = null; reloadThemes() }}
          />
        {/if}

        <section class="block">
          <label class="row">
            <input type="checkbox" checked={settings.autoSave} onchange={onToggle} />
            {t('settings.autoSaveLabel')}
          </label>
          {#if !isIOSPlatform}
            <label class="row">
              <input
                type="checkbox"
                bind:checked={settings.dailyNotes.enabled}
                onchange={async () => {
                  await saveSettings()
                  try {
                    const { invoke } = await import('@tauri-apps/api/core')
                    await invoke('set_daily_notes_enabled', { enabled: settings.dailyNotes.enabled })
                  } catch (e) { console.warn('[settings] set_daily_notes_enabled:', e) }
                }}
              />
              {t('settings.dailyNotes.label')}
            </label>
            <label class="row" style="margin-top: 6px;">
              <input
                type="checkbox"
                bind:checked={settings.mcpServer.enabled}
                onchange={async () => {
                  await saveSettings()
                  try {
                    await invoke('set_mcp_enabled', { enabled: settings.mcpServer.enabled })
                  } catch (e) { console.warn('[settings] set_mcp_enabled:', e) }
                }}
              />
              {t('settings.mcp.enable')}
            </label>
            <p class="desc" style="margin-top: 4px;">
              {t('settings.mcp.hint')} <code>{'{"command": "notemd", "args": ["mcp"]}'}</code>
            </p>
          {/if}
        </section>

        <section class="block">
          <h3>{t('vaultSync.title')}</h3>
          <p class="desc">{t('vaultSync.desc')}</p>
          <label class="row">
            <span class="lbl">{t('vaultSync.vaultPath')}</span>
            <input type="text" readonly
              value={vaultSettings.vaultPath ?? t('vaultSync.notConfigured')} />
          </label>
          <label class="row">
            <span class="lbl">{t('vaultSync.relPath')}</span>
            <input type="text" bind:value={syncDirDraft}
              placeholder={DEFAULT_SYNC_DIR}
              disabled={!vaultSettings.vaultPath || syncDirBusy} />
            <button onclick={onSaveSyncDir}
              disabled={!vaultSettings.vaultPath || syncDirBusy}>{t('vaultSync.save')}</button>
          </label>
          <label class="row">
            <span class="lbl">{t('vaultSync.largeFileThreshold')}</span>
            <input type="number" min="1" step="1" bind:value={thresholdDraft}
              disabled={!vaultSettings.vaultPath || thresholdBusy} />
            <button onclick={onSaveThreshold}
              disabled={!vaultSettings.vaultPath || thresholdBusy}>{t('vaultSync.save')}</button>
          </label>
          {#if agentsSectionMissing}
            <label class="row" style="align-items: flex-start;">
              <span class="lbl"></span>
              <div style="flex: 1; display: flex; flex-direction: column; gap: 4px;">
                <p class="desc" style="margin: 0;">{t('search.agentsHint')}</p>
                <button onclick={onAddAgentsSearchSection}
                  style="align-self: flex-start;"
                  disabled={agentsSectionBusy}>{t('search.agentsAdd')}</button>
              </div>
            </label>
          {/if}
        </section>

        {#if !isIOSPlatform}
          <section class="block">
            <h3>{t('settings.defaultApp.heading')}</h3>
            <p class="desc">{@html t('settings.defaultApp.desc1')}</p>
            <p class="desc">{@html t('settings.defaultApp.desc2', { exts: ALL_EXTS.length, groups: FILE_GROUPS.length })}</p>
            <details class="ext-list">
              <summary>{t('settings.defaultApp.showTypes', { count: ALL_EXTS.length })}</summary>
              <ul>
                {#each FILE_GROUPS as g}
                  <li><strong>{g.label}</strong> — {g.exts.map((e) => `.${e}`).join(', ')}</li>
                {/each}
              </ul>
            </details>
            <button class="primary" onclick={handleSetDefault} disabled={busy}>
              {busy ? t('settings.defaultApp.setting') : t('settings.defaultApp.setDefault', { count: ALL_EXTS.length })}
            </button>
            {#if resultText}
              <p
                class="result"
                class:ok={resultDetails?.every((r) => r.ok)}
                class:partial={resultDetails && resultDetails.some((r) => !r.ok) && resultDetails.some((r) => r.ok)}
                class:fail={resultDetails && resultDetails.every((r) => !r.ok)}
              >
                {resultText}
              </p>
            {/if}
            <p class="undo-note">{@html t('settings.defaultApp.undoNote')}</p>
          </section>

          <section class="block">
            <h3>{t('settings.proxy.title')}</h3>
            <p class="desc">{t('settings.proxy.desc')}</p>
            <div class="row" style="gap: 8px;">
              <input
                id="git-proxy"
                type="text"
                style="flex: 1;"
                placeholder="http://127.0.0.1:1080"
                bind:value={gitProxyInput}
                onkeydown={(e) => { if (e.key === 'Enter') void onSaveGitProxy() }}
              />
              <button onclick={() => void onSaveGitProxy()} disabled={gitProxyBusy}>
                {gitProxyBusy ? t('settings.proxy.saving') : t('settings.proxy.save')}
              </button>
            </div>
            {#if gitProxyError}
              <p class="result fail">{gitProxyError}</p>
            {:else if gitProxySaved}
              <p class="result ok">{t('settings.proxy.saved')}</p>
            {/if}
            <p class="undo-note">{t('settings.proxy.sshNote')}</p>
          </section>
        {/if}
      {:else if selectedTab === 'block'}
        <section class="block">
          <label class="row">
            <input type="checkbox" bind:checked={settings.mdblock.enabled} onchange={() => saveSettings()} />
            {t('settings.block.enable')}
          </label>
          <p class="desc">{@html t('settings.block.enableDesc')}</p>
        </section>

        <section class="block">
          <p class="desc">{@html t('settings.block.savingDesc')}</p>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.injectAiHint}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {@html t('settings.block.injectHint')}
          </label>
        </section>

        <section class="block">
          <h3>{t('settings.chunk.heading')}</h3>
          <label class="row">
            <span class="lbl">{t('settings.chunk.strategy')}</span>
            <select bind:value={settings.mdblock.chunkStrategy}
                    disabled={!settings.mdblock.enabled}
                    onchange={() => saveSettings()}>
              <option value="section">{t('settings.chunk.sectionFirst')}</option>
              <option value="size">{t('settings.chunk.sizeFirst')}</option>
            </select>
          </label>
          <p class="desc">{@html t('settings.chunk.sectionDesc')}</p>
          <label class="row" style:opacity={settings.mdblock.chunkStrategy === 'section' ? 1 : 0.5}>
            <span class="lbl">{t('settings.chunk.sectionCutLevel')}</span>
            <select bind:value={settings.mdblock.sectionCutLevel}
                    disabled={!settings.mdblock.enabled || settings.mdblock.chunkStrategy !== 'section'}
                    onchange={() => saveSettings()}>
              <option value={1}>{t('settings.chunk.h1opt')}</option>
              <option value={2}>{t('settings.chunk.h2opt')}</option>
              <option value={3}>{t('settings.chunk.h3opt')}</option>
            </select>
          </label>
          <label class="row" style:opacity={settings.mdblock.chunkStrategy === 'section' ? 1 : 0.5}>
            <span class="lbl">{t('settings.chunk.minChars')}</span>
            <input type="number" min="0" max="5000" step="50"
                   bind:value={settings.mdblock.sectionMinChars}
                   disabled={!settings.mdblock.enabled || settings.mdblock.chunkStrategy !== 'section'}
                   onchange={() => saveSettings()} />
          </label>
          <label class="row">
            <span class="lbl">{t('settings.chunk.maxChars')}</span>
            <input type="number" min="200" max="20000" step="100"
                   bind:value={settings.mdblock.chunkSizeChars}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
          </label>
          <p class="desc">{t('settings.chunk.maxCharsDesc')}</p>
          <label class="row">
            <span class="lbl">{t('settings.chunk.similarity')}</span>
            <input type="number" min="0" max="1" step="0.05"
                   bind:value={settings.mdblock.similarityThreshold}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
          </label>
        </section>

        <p class="desc">{@html t('settings.chunk.affectNote')}</p>

        <section class="block">
          <h3>{t('settings.viz.heading')}</h3>
          <p class="desc">{t('settings.viz.desc')}</p>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.hover.showSourceGutter}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {t('settings.viz.sourceMarkers')}
          </label>
          <label class="row">
            <input type="checkbox"
                   bind:checked={settings.mdblock.hover.showRichOverlay}
                   disabled={!settings.mdblock.enabled}
                   onchange={() => saveSettings()} />
            {t('settings.viz.richGutter')}
          </label>
        </section>
      {:else if selectedTab === 'cli'}
        <section class="block">
          <h3>{t('settings.cli.heading')}</h3>
          <p class="desc">{@html t('settings.cli.desc')}</p>
          {#if cliStatus === null}
            <p class="desc">{t('settings.cli.loading')}</p>
          {:else if cliStatus.installed}
            <p class="desc">
              {t('settings.cli.installedAtLabel')} <code>{cliStatus.path}</code>
              {#if !cliStatus.target_valid}
                <br />
                <span style="color: tomato;">{t('settings.cli.symlinkMismatch')}</span>
              {/if}
            </p>
            <div class="row" style="gap: 8px; flex-wrap: wrap;">
              <button onclick={handleCliInstall} disabled={cliBusy}>
                {cliBusy ? t('settings.cli.working') : t('settings.cli.reinstall')}
              </button>
              <button onclick={handleCliUninstall} disabled={cliBusy}>
                {cliBusy ? t('settings.cli.working') : t('settings.cli.uninstall')}
              </button>
            </div>
          {:else}
            <p class="desc">{t('settings.cli.notInstalled')}</p>
            <button class="primary" onclick={handleCliInstall} disabled={cliBusy}>
              {cliBusy ? t('settings.cli.installing') : t('settings.cli.install')}
            </button>
          {/if}
          {#if cliError}
            <p class="result fail">{t('settings.cli.error', { error: cliError })}</p>
          {/if}
          <p class="desc" style="margin-top: 12px;">{@html t('settings.cli.helpDesc')}</p>
        </section>
      {:else if selectedTab === 'updates'}
        <section class="block">
          <h3>{t('settings.update.heading')}</h3>
          <p class="desc">
            {t('settings.update.currentVersionLabel')}<strong>v{updaterState.currentVersion || '—'}</strong>
          </p>
          <p class="desc">
            {t('settings.update.lastChecked', { time: formatLastChecked(updaterState.lastCheckedAt) })}
          </p>
          <label class="row" style="margin-top: 8px;">
            <input type="checkbox" checked={updaterState.checkOnStartup} onchange={onCheckOnStartupToggle} />
            {t('settings.update.autoCheck')}
          </label>
          <div class="row" style="gap: 8px; flex-wrap: wrap; margin-top: 10px;">
            <button onclick={handleCheckUpdate} disabled={updaterBusy || updaterState.state === 'checking' || updaterState.state === 'downloading'}>
              {updaterState.state === 'checking' ? t('settings.update.checking') : t('settings.update.checkNow')}
            </button>
            {#if hasUpdateForSettings() && updaterState.state !== 'downloading' && updaterState.state !== 'ready'}
              <button class="primary" onclick={handleUpdateNow} disabled={updaterBusy}>
                {t('settings.update.downloadInstall', { version: updaterState.latestVersion ?? '' })}
              </button>
            {/if}
            {#if updaterState.state === 'ready'}
              <button class="primary" onclick={handleRestart}>{t('settings.update.restartNow')}</button>
            {/if}
          </div>

          {#if updaterState.state === 'downloading'}
            <p class="desc" style="margin-top: 10px;">
              {t('settings.update.downloading')}
              {#if updaterState.contentLength}
                {Math.round((updaterState.downloaded / updaterState.contentLength) * 100)}%
              {:else}
                {(updaterState.downloaded / 1024 / 1024).toFixed(1)} MB
              {/if}
            </p>
          {/if}

          {#if updaterMessage}
            <p class="result" class:ok={updaterState.state === 'uptodate'} class:fail={updaterState.state === 'error'}>
              {updaterMessage}
            </p>
          {/if}

          {#if updaterState.state === 'available' && updaterState.notes}
            <details style="margin-top: 12px;">
              <summary>{t('settings.update.notes', { version: updaterState.latestVersion ?? '' })}</summary>
              <pre style="margin-top: 8px; white-space: pre-wrap; word-break: break-word; background: color-mix(in srgb, CanvasText 5%, transparent); padding: 8px; border-radius: 6px; font-size: 11px;">{updaterState.notes}</pre>
            </details>
          {/if}

          <p class="desc" style="margin-top: 14px;">
            {t('settings.update.distNote')}
          </p>
        </section>
      {:else if selectedTab === 'vault' && isIOSPlatform}
        <VaultSettingsTab />
      {:else if selectedTab === 'search'}
        <section class="block">
          <h3>{t('smartSearch.windowTitle')}</h3>
          <p class="desc">{t('smartSearch.smartUnderstandingHint')}</p>
          <label class="row">
            <span class="lbl">{t('smartSearch.smartUnderstanding')}</span>
            <input type="checkbox" checked={settings.smartLookup.planner.enabled} onchange={(event) => {
              settings.smartLookup.planner.enabled = event.currentTarget.checked; void saveSettings()
            }} />
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.plannerProvider')}</span>
            <select value={settings.smartLookup.planner.provider} onchange={(event) => {
              settings.smartLookup.planner.provider = event.currentTarget.value; void saveSettings()
            }}>
              <option value="auto">{t('smartSearch.auto')}</option>
              {#each smartLookupPlannerAgents as agent (agent.id)}
                <option value={agent.id}>{agent.harness?.harness || agent.name}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.resultLimit')}</span>
            <select value={settings.smartLookup.results.limit} onchange={(event) => {
              settings.smartLookup.results.limit = Number(event.currentTarget.value) as 20 | 50 | 100; void saveSettings()
            }}>
              <option value="20">20</option><option value="50">50</option><option value="100">100</option>
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.groupBy')}</span>
            <select value={settings.smartLookup.results.groupBy} onchange={(event) => {
              settings.smartLookup.results.groupBy = event.currentTarget.value as 'auto' | 'source' | 'date'; void saveSettings()
            }}>
              <option value="auto">{t('smartSearch.auto')}</option>
              <option value="source">{t('smartSearch.groupSource')}</option>
              <option value="date">{t('smartSearch.groupDate')}</option>
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.autoDeep')}</span>
            <input type="checkbox" checked={settings.smartLookup.results.autoDeepOnZero} onchange={(event) => {
              settings.smartLookup.results.autoDeepOnZero = event.currentTarget.checked; void saveSettings()
            }} />
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.quickSummary')}</span>
            <input type="checkbox" checked={settings.smartLookup.summary.enabled} onchange={(event) => {
              settings.smartLookup.summary.enabled = event.currentTarget.checked; void saveSettings()
            }} />
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.summaryStyle')}</span>
            <select value={settings.smartLookup.summary.style} onchange={(event) => {
              settings.smartLookup.summary.style = event.currentTarget.value as 'sentence' | 'bullets'; void saveSettings()
            }}>
              <option value="bullets">{t('smartSearch.summaryBullets')}</option>
              <option value="sentence">{t('smartSearch.summarySentence')}</option>
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.handoffProvider')}</span>
            <select value={settings.smartLookup.handoff.defaultProvider} onchange={(event) => {
              settings.smartLookup.handoff.defaultProvider = event.currentTarget.value; void saveSettings()
            }}>
              <option value="ask">{t('smartSearch.askEveryTime')}</option>
              {#each smartLookupHandoffAgents as agent (agent.id)}
                <option value={agent.id}>{agent.harness?.harness || agent.name}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span class="lbl">{t('smartSearch.includeRefs')}</span>
            <input type="checkbox" checked={settings.smartLookup.handoff.includeSelectedRefs} onchange={(event) => {
              settings.smartLookup.handoff.includeSelectedRefs = event.currentTarget.checked; void saveSettings()
            }} />
          </label>
        </section>
        <section class="block">
          <h3>{t('settings.tab.search')}</h3>
          {#if !sotvaultStore.vaultRoot}
            <p class="desc">{t('search.index.noVault')}</p>
          {:else}
            {#if indexStatus.notReady}
              <!-- Two very different situations used to render as one
                   sentence here, with the whole stats block (Rebuild button
                   included) hidden behind the `{:else}`: a scan that clears
                   itself in seconds-to-minutes, and an open that FAILED and
                   will never retry on its own. The failed case has to name
                   its reason and offer the one action that actually recovers
                   it — reopening, not rebuilding (a rebuild needs the index
                   handle the failed open never installed). Live progress for
                   the building case renders in its own section below, driven
                   by the startup build's progress callback. -->
              {#if indexStatus.openFailed}
                <p class="result fail">
                  {t('search.index.openFailed', { error: indexStatus.openState?.error ?? '' })}
                </p>
                <div class="row" style="margin-top: 10px;">
                  <span class="lbl"></span>
                  <button onclick={() => void indexStatus.requestReopen()}>{t('search.index.retryOpen')}</button>
                </div>
                {#if indexStatus.reopenError}
                  <p class="result fail">{t('search.index.reopenError', { error: indexStatus.reopenError })}</p>
                {/if}
              {:else}
                <p class="desc">{t('search.notReady')}</p>
                {#if !indexStatus.progress}
                  <!-- No progress event yet (the walk hasn't reported, or the
                       host predates the startup-build callback). Say what is
                       being waited on rather than leaving a bare sentence
                       that looks identical to a permanent failure. -->
                  <p class="desc">{t('search.index.buildingHint')}</p>
                {/if}
              {/if}
            {:else if indexStatus.error}
              <p class="result fail">{indexStatus.error}</p>
            {:else}
              <div class="row">
                <span class="lbl">{t('search.index.files')}</span>
                <span>{indexStatus.stats ? indexStatus.stats.files : (indexStatus.loading ? '…' : '—')}</span>
              </div>
              <div class="row">
                <span class="lbl">{t('search.index.blocks')}</span>
                <span>{indexStatus.stats ? indexStatus.stats.blocks : (indexStatus.loading ? '…' : '—')}</span>
              </div>
              <div class="row">
                <span class="lbl">{t('search.index.dbSize')}</span>
                <span>{indexStatus.stats ? formatBytes(indexStatus.stats.dbBytes) : (indexStatus.loading ? '…' : '—')}</span>
              </div>
              <div class="row">
                <span class="lbl">{t('search.index.builtAt')}</span>
                <span>{indexStatus.stats?.builtAt ?? (indexStatus.loading ? '…' : t('time.never'))}</span>
              </div>
              <div class="row">
                <span class="lbl">{t('search.index.tokenizer')}</span>
                <span>{indexStatus.stats ? indexStatus.stats.tokenizerId : (indexStatus.loading ? '…' : '—')}</span>
              </div>
              <div class="row" style="margin-top: 10px;">
                <span class="lbl"></span>
                <button onclick={() => void onRebuildIndex()} disabled={indexStatus.progress !== null}>
                  {t('search.rebuild')}
                </button>
              </div>
              {#if indexStatus.busyNotice}
                <p class="result">{t('search.index.busyNotice')}</p>
              {/if}
              {#if indexStatus.rebuildError}
                <p class="result fail">{t('search.index.rebuildError', { error: indexStatus.rebuildError })}</p>
              {/if}
            {/if}
            <!-- Independent of the notReady/error branches above — the log
                 window is exactly where you'd look to understand *why* the
                 index is stuck or erroring, so it must stay reachable there
                 too (design spec §7). -->
            <div class="row" style="margin-top: 10px;">
              <span class="lbl"></span>
              <button onclick={() => void onViewSearchLogs()}>{t('search.index.viewLogs')}</button>
            </div>
          {/if}
        </section>
        {#if sotvaultStore.vaultRoot}
          <section class="block">
            <h3>{t('search.index.tiersHeading')}</h3>
            {#if indexStatus.stats}
              {@const oc = indexStatus.stats.originCounts}
              {@const tc = indexStatus.stats.typeCounts}
              {@const namedDerivedTotal = Object.values(tc).reduce((a, b) => a + b, 0)}
              {@const untypedDerived = Math.max(0, oc.derived - namedDerivedTotal)}
              <div class="row">
                <span class="lbl">{t('search.group.human')}</span>
                <span>{oc.human}</span>
              </div>
              <div class="row">
                <span class="lbl">{t('search.index.tiersDerivedLabel')}</span>
                <span>{oc.derived}</span>
              </div>
              <!-- Named types are the raw `concept_type` string, deliberately
                   untranslated — same ruling as the search panel's middle-band
                   group headers (spec §5, `src/lib/search/grouping.ts`). -->
              {#each Object.entries(tc).sort((a, b) => b[1] - a[1]) as [type, n] (type)}
                <div class="row" style="padding-left: 16px;">
                  <span class="lbl">{type}</span>
                  <span>{n}</span>
                </div>
              {/each}
              {#if untypedDerived > 0}
                <div class="row" style="padding-left: 16px;">
                  <span class="lbl">{t('search.group.other')}</span>
                  <span>{untypedDerived}</span>
                </div>
              {/if}
              <div class="row">
                <span class="lbl">{t('search.group.source')}</span>
                <span>{oc.source}</span>
              </div>
              <!-- Design spec §7.4: the ONE clickable, actionable statistic
                   row — the designed exit from the ×0.3 demotion (spec §3.1).
                   `role="button"`/`tabindex`/`onkeydown` make it reachable
                   without a mouse; `class="tier-unlabeled"` is a dedicated
                   hook (component tests locate it directly rather than by
                   label text, since the row is also asserted as a plain
                   (label, value) pair like every other tier row above). -->
              <div
                class="row tier-unlabeled"
                role="button"
                tabindex="0"
                onclick={() => void onUnlabeledTierClick()}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); void onUnlabeledTierClick() } }}
              >
                <span class="lbl">{t('search.group.unlabeled')}</span>
                <span>{oc.unlabeled}</span>
              </div>
              <p class="desc" style="margin-top: 8px;">{t('search.index.tiersHint')}</p>
              <p class="desc">{t('search.index.tiersUnlabeledHint', { label: t('search.group.unlabeled') })}</p>
              <!-- Coverage row: ingestion "just not having run" has no visible
                   symptom anywhere else — search silently degrades to
                   unweighted results. This is the only discovery path, so it
                   must stay even though it's plain. `attentionAsOf === null`
                   means ingestion never ran on this index (row hidden,
                   nothing to report yet); once it has a value, the row shows
                   even when `attentionFiles` is 0 — that combination is the
                   most important diagnostic signal, not a case to hide. -->
              {#if indexStatus.stats.attentionAsOf}
                <div class="row">
                  <span class="lbl">{t('search.index.attentionLabel')}</span>
                  <span>{indexStatus.stats.attentionFiles} / {indexStatus.stats.files}</span>
                </div>
              {/if}
            {:else}
              <p class="desc">{indexStatus.loading ? '…' : '—'}</p>
            {/if}
          </section>
        {/if}
        {#if indexStatus.progress}
          {@const p = indexStatus.progress}
          {@const percent = progressPercent(p)}
          <section class="block">
            <h3>{t('search.index.progressHeading')}</h3>
            <p class="desc">{t('search.index.progressLine', { phase: phaseLabel(p.phase), done: p.done, total: p.total, percent })}</p>
            <div class="progress">
              <div class="bar" style:width={`${percent}%`}></div>
            </div>
            {#if p.current}
              <div class="row" style="margin-top: 8px;">
                <span class="lbl">{t('search.index.currentFile')}</span>
                <span title={p.current}>{elideMiddle(p.current)}</span>
              </div>
            {/if}
            <p class="desc" style="margin-top: 6px;">{t('search.index.elapsed', { time: formatElapsedMs(p.elapsedMs) })}</p>
          </section>
        {/if}
        {#if sotvaultStore.vaultRoot}
          <section class="block">
            <h3>{t('search.index.settingsHeading')}</h3>
            <label class="row" style="align-items: flex-start;">
              <span class="lbl">{t('settings.searchExcludeDirs')}</span>
              <div style="flex: 1; display: flex; flex-direction: column; gap: 4px;">
                <textarea rows="3" bind:value={searchExcludeDirsDraft}
                  disabled={!vaultSettings.vaultPath || searchExcludeDirsBusy}
                  style="width: 100%; font: inherit; padding: 6px; border-radius: 4px; border: 1px solid color-mix(in srgb, CanvasText 25%, transparent); background: Canvas; color: CanvasText; resize: vertical;"
                ></textarea>
                <p class="desc" style="margin: 0;">{t('settings.searchExcludeDirsHint')}</p>
                <button onclick={onSaveSearchExcludeDirs}
                  style="align-self: flex-start;"
                  disabled={!vaultSettings.vaultPath || searchExcludeDirsBusy}>{t('vaultSync.save')}</button>
              </div>
            </label>
            <label class="row" style="align-items: flex-start;">
              <span class="lbl">{t('search.index.thresholdLabel')}</span>
              <div style="flex: 1; display: flex; flex-direction: column; gap: 4px;">
                <div class="row" style="gap: 8px;">
                  <input type="number" min="1" step="1" bind:value={searchThresholdDraft}
                    disabled={!vaultSettings.vaultPath || searchThresholdBusy} />
                  <button onclick={onSaveSearchThreshold}
                    disabled={!vaultSettings.vaultPath || searchThresholdBusy}>{t('vaultSync.save')}</button>
                </div>
                <p class="desc" style="margin: 0;">{t('search.index.thresholdHint')}</p>
              </div>
            </label>
          </section>
          <section class="block">
            <h3>{t('search.index.globsHeading')}</h3>
            <p class="desc">{t('search.index.globsHint')}</p>
            {#if globRows.length === 0}
              {#if vaultSettings.searchSourceGlobs === null}
                <!-- Review round 1, Important 3: an unconfigured vault is
                     NOT the same state as an explicitly-saved empty list —
                     it already has an effective pattern (the implicit
                     `<syncDir>/**` seed). Showing that here is what lets the
                     user see what they're about to replace before Save
                     silently un-seeds it. -->
                <p class="desc">{t('search.index.globsImplicitDefault', { pattern: effectiveDefaultGlob })}</p>
              {:else}
                <p class="result fail">{t('search.index.globsExplicitlyEmpty')}</p>
              {/if}
            {/if}
            {#each globRows as row, i (i)}
              <div class="row glob-row" style="gap: 8px; margin-bottom: 4px;">
                <input type="text" style="flex: 1;" value={row.pattern}
                  oninput={(e) => onGlobRowInput(i, e.currentTarget.value)}
                  onblur={() => onGlobRowBlur(i)}
                  placeholder={t('search.index.globsPatternPlaceholder')}
                  disabled={globSaveBusy} />
                <!-- Review round 1, Important 2: the SAVED list — what
                     actually ships into the index — must show the same
                     real-vault count the candidate ladder above already
                     shows, not just a silent zero-match warning. -->
                <span class="glob-count">{formatMatchCount(row.count)}</span>
                <button onclick={() => onRemoveGlobRow(i)} disabled={globSaveBusy}>{t('search.index.globsRemoveRow')}</button>
              </div>
              {#if row.count === 0}
                <p class="result fail" style="margin: 0 0 6px 0;">{t('search.index.globsZeroMatchWarning')}</p>
              {/if}
            {/each}
            <div class="row" style="gap: 8px; margin-top: 6px;">
              <button onclick={onAddGlobRow} disabled={globSaveBusy}>{t('search.index.globsAddRow')}</button>
              <button class="primary" onclick={() => void onSaveSourceGlobs()} disabled={globSaveBusy || !vaultSettings.vaultPath}>{t('vaultSync.save')}</button>
            </div>
            {#if globPatternBlankError}
              <p class="result fail">{globPatternBlankError}</p>
            {/if}
            <p class="desc" style="margin-top: 10px;">{t('search.index.globsRebuildNote')}</p>

            <h3 style="margin-top: 16px;">{t('search.index.globsSampleHeading')}</h3>
            <div class="row" style="gap: 8px;">
              <input type="text" style="flex: 1;" bind:value={globSampleDraft}
                placeholder={t('search.index.globsSamplePlaceholder')} />
              <button onclick={onGenerateGlobCandidates} disabled={!globSampleDraft.trim()}>{t('search.index.globsSampleGenerate')}</button>
            </div>
            {#if globCandidates.length > 0}
              <div class="glob-candidates" style="margin-top: 8px; display: flex; flex-direction: column; gap: 6px;">
                {#each globCandidates as c (c.pattern)}
                  <label class="row" style="gap: 8px;">
                    <input type="radio" name="glob-candidate" value={c.pattern}
                      checked={globCandidateSelected === c.pattern}
                      onchange={() => (globCandidateSelected = c.pattern)} />
                    <code style="flex: 1;">{c.pattern}</code>
                    <span>{formatMatchCount(c.count)}</span>
                  </label>
                {/each}
                <button class="primary" style="align-self: flex-start;" onclick={onUseSelectedCandidate}>{t('search.index.globsCandidateUse')}</button>
              </div>
            {/if}
          </section>
          <section class="block">
            <h3>{t('search.weights.heading')}</h3>
            <p class="desc">{t('search.weights.hint')}</p>
            <div class="row">
              <span class="lbl">{t('search.group.human')}</span>
              <input type="number" min="0.01" max="5" step="0.05" bind:value={weightsDraft.human} oninput={() => (weightsError = null)} disabled={weightsBusy} />
            </div>
            <div class="row">
              <span class="lbl">{t('search.index.tiersDerivedLabel')}</span>
              <input type="number" min="0.01" max="5" step="0.05" bind:value={weightsDraft.derived} oninput={() => (weightsError = null)} disabled={weightsBusy} />
            </div>
            <div class="row">
              <span class="lbl">{t('search.group.source')}</span>
              <input type="number" min="0.01" max="5" step="0.05" bind:value={weightsDraft.source} oninput={() => (weightsError = null)} disabled={weightsBusy} />
            </div>
            <div class="row">
              <span class="lbl">{t('search.group.unlabeled')}</span>
              <input type="number" min="0.01" max="5" step="0.05" bind:value={weightsDraft.unlabeled} oninput={() => (weightsError = null)} disabled={weightsBusy} />
            </div>
            <div class="row" style="gap: 8px; margin-top: 8px;">
              <button onclick={onResetWeightsDraft} disabled={weightsBusy}>{t('search.weights.resetDefault')}</button>
              <button class="primary" onclick={() => void onSaveWeights()} disabled={weightsBusy || !vaultSettings.vaultPath}>{t('vaultSync.save')}</button>
            </div>
            {#if weightsError}
              <p class="result fail">{weightsError}</p>
            {/if}
            <p class="desc" style="margin-top: 8px;">{t('search.weights.rebuildContrastNote')}</p>
          </section>
          <section class="block">
            <h3>{t('search.index.skippedHeading')}</h3>
            {#if !indexStatus.stats || indexStatus.stats.skippedLarge.length === 0}
              <p class="desc">{t('search.index.skippedEmpty')}</p>
            {:else}
              {#each indexStatus.stats.skippedLarge as f (f.path)}
                <div class="row">
                  <span class="lbl" title={f.path}>{elideMiddle(f.path)}</span>
                  <span>{formatBytes(f.sizeBytes)}</span>
                </div>
              {/each}
              <p class="desc" style="margin-top: 4px;">{t('search.index.skippedNote')}</p>
            {/if}
          </section>
        {/if}
      {:else if selectedTab === 'outline-notes'}
        <section class="block">
          <h3>{t('outline.shortcutsTitle')}</h3>
          {#each Object.keys(DEFAULT_SHORTCUTS) as id (id)}
            <div class="shortcut-row">
              <span class="shortcut-label">{t(OUTLINE_CMD_LABELS[id as OutlineCommandId])}</span>
              <button
                class="shortcut-input" class:recording={recording === id}
                onclick={() => (recording = id as OutlineCommandId)}
                onkeydown={(e) => recording === id && onRecordKey(e, id as OutlineCommandId)}
                onblur={() => { if (recording === id) recording = null }}
              >
                {recording === id ? t('outline.pressKeys') : displayShortcut(resolvedOutline[id as OutlineCommandId], isMac)}
              </button>
              {#if outlineShortcuts.overrides[id as OutlineCommandId]}
                <button class="shortcut-reset" onclick={() => void setShortcutOverride(id as OutlineCommandId, null)}>↺</button>
              {/if}
            </div>
          {/each}
          {#if conflictMsg}<p class="shortcut-conflict">{conflictMsg}</p>{/if}
        </section>
        <section class="block">
          <h3>{t('outline.dirsTitle')}</h3>
          <div class="field-row">
            <label for="wikipage-dir">{t('outline.wikipageDir')}</label>
            <input id="wikipage-dir" type="text" value={outlineDirs.wikipage}
              onchange={(e) => void onSetOutlineDir('wikipage', (e.currentTarget as HTMLInputElement).value)} />
          </div>
          <div class="field-row">
            <label for="dailynote-dir">{t('outline.dailynoteDir')}</label>
            <input id="dailynote-dir" type="text" value={outlineDirs.dailynote}
              onchange={(e) => void onSetOutlineDir('dailynote', (e.currentTarget as HTMLInputElement).value)} />
          </div>
          <div class="field-row">
            <label for="inbox-dir">{t('quickNote.inboxDir')}</label>
            <input id="inbox-dir" type="text" value={inboxDir.value}
              onchange={(e) => void onSetInboxDir((e.currentTarget as HTMLInputElement).value)} />
          </div>
        </section>
      {:else}
        {#each pluginTabs as ptab (ptab.pluginId)}
          {#if selectedTab === ptab.pluginId}
            <div class="plugin-settings">
              {#each ptab.schema as field (field.key)}
                {@const localKey = field.key.slice(ptab.pluginId.length + 1)}
                <label class="plugin-field">
                  <span class="lbl">{pluginFieldLabel(ptab.manifest, field.key, field.label)}</span>
                  {#if field.type === 'string'}
                    <input type="text"
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? field.default ?? ''}
                      placeholder={field.placeholder ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).value)} />
                  {:else if field.type === 'secret'}
                    <input type="password"
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).value)} />
                  {:else if field.type === 'select'}
                    <select
                      value={(pluginValues[ptab.pluginId]?.[localKey] as string) ?? field.default ?? ''}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLSelectElement).value)}>
                      {#each field.options as opt}
                        <option value={opt}>{opt}</option>
                      {/each}
                    </select>
                  {:else if field.type === 'boolean'}
                    <input type="checkbox"
                      checked={(pluginValues[ptab.pluginId]?.[localKey] as boolean) ?? field.default ?? false}
                      onchange={(e) => savePluginField(ptab.pluginId, localKey, (e.currentTarget as HTMLInputElement).checked)} />
                  {:else if field.type === 'number'}
                    <input type="number"
                      value={(pluginValues[ptab.pluginId]?.[localKey] as number) ?? field.default ?? ''}
                      min={field.min}
                      max={field.max}
                      step={field.step}
                      onchange={(e) => savePluginNumberField(ptab.pluginId, localKey, e.currentTarget as HTMLInputElement)} />
                  {/if}
                </label>
              {/each}
            </div>
          {/if}
        {/each}
      {/if}

      <div class="actions">
        <button onclick={() => (open = false)}>{t('settings.done')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed; inset: 0;
    background: rgba(0,0,0,0.3);
    display: flex; align-items: center; justify-content: center;
    z-index: 100;
  }
  .dialog {
    width: min(560px, 92vw);
    max-height: 86vh;
    overflow: auto;
    background: Canvas;
    color: CanvasText;
    border: 1px solid color-mix(in srgb, CanvasText 20%, transparent);
    border-radius: 8px;
    padding: 18px 20px;
    box-shadow: 0 12px 40px rgba(0,0,0,0.25);
  }
  h2 { margin: 0 0 12px 0; font-size: 16px; }
  h3 {
    margin: 0 0 8px 0;
    font-size: 13px;
    font-weight: 600;
  }
  .block {
    padding: 12px 0;
    border-top: 1px solid color-mix(in srgb, CanvasText 10%, transparent);
  }
  .block:first-of-type { border-top: 0; padding-top: 0; }
  .row { display: flex; gap: 8px; align-items: center; font-size: 13px; }
  .row .lbl {
    width: 60px;
    flex-shrink: 0;
  }
  .row select {
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 13px;
    flex: 1;
    max-width: 240px;
  }
  .desc {
    font-size: 12px;
    line-height: 1.5;
    margin: 0 0 8px 0;
    color: color-mix(in srgb, CanvasText 75%, transparent);
  }
  .ext-list {
    margin: 8px 0;
    font-size: 12px;
  }
  .ext-list summary {
    cursor: pointer;
    color: color-mix(in srgb, CanvasText 65%, transparent);
    user-select: none;
  }
  .ext-list ul {
    margin: 8px 0 0 0;
    padding-left: 18px;
    line-height: 1.7;
    color: color-mix(in srgb, CanvasText 80%, transparent);
  }
  .ext-list li { font-family: ui-monospace, Menlo, monospace; font-size: 11px; }
  .ext-list li strong { font-family: -apple-system, system-ui, sans-serif; font-size: 12px; }
  .actions { display: flex; justify-content: flex-end; margin-top: 16px; }
  button {
    padding: 6px 14px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    cursor: pointer;
    font-size: 12px;
  }
  button:disabled { opacity: 0.5; cursor: progress; }
  .primary {
    background: AccentColor;
    color: AccentColorText;
    border-color: AccentColor;
    font-weight: 500;
  }
  .result {
    margin: 10px 0 0 0;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    line-height: 1.45;
    background: color-mix(in srgb, CanvasText 8%, transparent);
  }
  .result.ok {
    background: color-mix(in srgb, #2ea043 18%, transparent);
    color: color-mix(in srgb, #2ea043 90%, CanvasText);
  }
  .result.partial {
    background: color-mix(in srgb, #d29922 22%, transparent);
  }
  .result.fail {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }
  /* Design spec §7.4: the only clickable statistic row — the exit from the
     unlabeled tier's ×0.3 demotion. */
  .tier-unlabeled {
    cursor: pointer;
    border-radius: 4px;
    margin: 0 -6px;
    padding: 2px 6px;
  }
  .tier-unlabeled:hover, .tier-unlabeled:focus-visible {
    background: color-mix(in srgb, AccentColor 12%, transparent);
    outline: none;
  }
  .glob-row input[type="text"] {
    padding: 6px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 13px;
  }
  .undo-note {
    margin: 10px 0 0 0;
    font-size: 11px;
    line-height: 1.5;
    color: color-mix(in srgb, CanvasText 60%, transparent);
  }
  .progress {
    height: 8px;
    margin-top: 8px;
    background: color-mix(in srgb, CanvasText 12%, transparent);
    border-radius: 999px;
    overflow: hidden;
  }
  .progress .bar {
    height: 100%;
    background: AccentColor;
    transition: width 0.2s ease-out;
  }
  .tab-strip {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 20%, transparent);
    margin-bottom: 12px;
  }
  .tab-strip button {
    background: transparent;
    border: 0;
    padding: 8px 12px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    font-size: 12px;
    color: CanvasText;
    border-radius: 0;
  }
  .tab-strip button.active {
    border-bottom-color: AccentColor;
    font-weight: 600;
  }
  .plugin-settings { display: flex; flex-direction: column; gap: 12px; padding: 4px 0; }
  .plugin-field { display: flex; align-items: center; gap: 12px; font-size: 13px; }
  .plugin-field .lbl { width: 160px; flex-shrink: 0; }
  .plugin-field input[type="text"],
  .plugin-field input[type="password"],
  .plugin-field input[type="number"],
  .plugin-field select {
    flex: 1;
    padding: 6px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 12px;
  }
  .shortcut-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 13px;
  }
  .shortcut-label {
    width: 120px;
    flex-shrink: 0;
  }
  .shortcut-input {
    font-family: ui-monospace, Menlo, monospace;
    font-size: 12px;
    padding: 4px 8px;
    min-width: 120px;
    text-align: center;
  }
  .shortcut-input.recording {
    outline: 1px solid AccentColor;
    color: AccentColor;
  }
  .shortcut-reset {
    padding: 4px 8px;
    font-size: 13px;
    opacity: 0.6;
  }
  .shortcut-reset:hover { opacity: 1; }
  .field-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 13px;
  }
  .field-row label {
    width: 140px;
    flex-shrink: 0;
  }
  .field-row input[type="text"] {
    flex: 1;
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
    background: Canvas;
    color: CanvasText;
    font-size: 12px;
    max-width: 200px;
  }
  .shortcut-conflict {
    color: #d44a4a;
    font-size: 12px;
    margin: 4px 0 0 0;
  }
</style>
