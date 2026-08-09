<script lang="ts">
  import { outline } from '../../lib/outline/store.svelte'
  import { recallCandidateFiles, recallGroupForFile, type RecallGroup } from '../../lib/outline/recall'
  import { openFile } from '../../lib/tabs.svelte'
  import { openPageOrCreate } from '../../lib/outline/backlinks-io.svelte'
  import { commitReferenceEdit } from '../../lib/outline/recall-writeback-io'
  import { t } from '../../lib/i18n/store.svelte'
  import RefTreeNode from './RefTreeNode.svelte'
  import { basename } from '../../lib/paths'

  let { page = null, excludeFile = null }: { page?: string | null; excludeFile?: string | null } = $props()

  const fileName = (path: string) => basename(path)

  let groups = $state<RecallGroup[]>([])
  let totalFiles = $state(0)                    // candidate files — known instantly (the frame)
  let loadedFiles = $state(0)                   // processed so far
  let loadingFile = $state<string | null>(null) // currently-processing file (flashes)
  const count = $derived(groups.reduce((n, g) => n + g.carriers.length, 0))
  const loading = $derived(loadedFiles < totalFiles)

  // Progressive load: candidate files come from the flat index instantly (frame
  // + count), then groups stream in one chunk per animation frame so a large
  // reference set fills in visibly instead of freezing the UI. A short debounce
  // coalesces rapid bumps; a token cancels superseded runs.
  let timer: ReturnType<typeof setTimeout> | undefined
  let raf = 0
  let token = 0

  function reset() { groups = []; totalFiles = 0; loadedFiles = 0; loadingFile = null }

  function start(idx: NonNullable<typeof outline.backlinkIndex>, p: string) {
    const mine = ++token
    if (raf) cancelAnimationFrame(raf)
    const files = recallCandidateFiles(idx, p, excludeFile ?? undefined)
    reset()
    totalFiles = files.length
    let i = 0
    const CHUNK = 6
    const step = () => {
      if (mine !== token) return
      const end = Math.min(i + CHUNK, files.length)
      for (; i < end; i++) {
        loadingFile = fileName(files[i])
        const g = recallGroupForFile(idx, p, files[i])
        if (g) groups.push(g)
      }
      loadedFiles = i
      if (i < files.length) raf = requestAnimationFrame(step)
      else { loadingFile = null; raf = 0 }
    }
    raf = requestAnimationFrame(step)
  }

  $effect(() => {
    void outline.version
    const p = page
    const idx = outline.backlinkIndex
    clearTimeout(timer)
    if (!p || !idx) { token++; reset(); return }
    timer = setTimeout(() => start(idx, p), 30)
    return () => { clearTimeout(timer); if (raf) cancelAnimationFrame(raf); token++ }
  })

  const openSource = (file: string) => void openFile(file)
  // B1: only outline-file sources are editable in place (safe parse↔serialize
  // round-trip); prose .md references stay read-only.
  const isOutlineFile = (f: string) => /\.notes?\.md$/i.test(f)
  // Wikilink / hashtag inside a reference navigates (same as the outline).
  const onPageClick = (target: string) => void openPageOrCreate(target)
</script>

{#if loading || count > 0}
  <section class="linked-refs" aria-busy={loading}>
    <header class="lr-head">
      <span class="lr-title">{#if count > 0}{count} {/if}{t('outline.linkedReferences')}</span>
      <span class="lr-actions">
        {#if loading}
          <span class="lr-loading">{t('outline.loading')} {loadedFiles}/{totalFiles}{#if loadingFile} · {loadingFile}{/if}</span>
        {:else}
          <button class="icon" title="Search" disabled aria-label="search">⌕</button>
          <button class="icon" title="Filter" disabled aria-label="filter">⚑</button>
          <button class="icon" title="More" disabled aria-label="more">⋯</button>
        {/if}
      </span>
    </header>

    {#each groups as g (g.file)}
      {@const editable = isOutlineFile(g.file)}
      <div class="lr-group">
        <button class="lr-file" onclick={() => void openFile(g.file)}>{fileName(g.file)}</button>
        {#each g.carriers as carrier, i (i)}
          <div class="lr-carrier">
            {#if carrier.breadcrumb.length}
              <button class="lr-crumb" onclick={() => openSource(g.file)} title={g.file}
                >/{carrier.breadcrumb.join('/')}</button>
            {/if}
            <RefTreeNode
              node={carrier.node}
              defaultCollapsed={true}
              {editable}
              {onPageClick}
              onCommit={(path, oldText, newText) => commitReferenceEdit(g.file, path, oldText, newText)}
            />
          </div>
        {/each}
      </div>
    {/each}
  </section>
{/if}

<style>
  /* Inherit the main outline's typography so the whole section reads as one
     continuous outline; header/breadcrumb are sized relative to it. */
  .linked-refs {
    margin-top: 20px;
    font-family: var(--outline-font-family);
    font-size: var(--outline-font-size, 13px);
    line-height: var(--outline-line-height, 1.5);
  }
  .lr-head {
    display: flex; align-items: center; justify-content: space-between;
    border-top: 1px solid var(--border-color, #3333);
    padding: 8px 2px 6px; margin-bottom: 4px;
  }
  .lr-title { font-size: 0.85em; font-weight: 600; opacity: 0.6; letter-spacing: 0.02em; }
  .lr-actions { display: flex; gap: 2px; align-items: center; }
  .lr-loading {
    font-size: 0.8em; opacity: 0.55; white-space: nowrap; max-width: 60%;
    overflow: hidden; text-overflow: ellipsis; animation: lr-pulse 1s ease-in-out infinite;
  }
  @keyframes lr-pulse { 0%, 100% { opacity: 0.35; } 50% { opacity: 0.7; } }
  .icon {
    background: none; border: none; color: inherit; cursor: pointer;
    font-size: 1em; opacity: 0.4; padding: 2px 5px; border-radius: 4px;
  }
  .icon:disabled { cursor: default; }
  .icon:not(:disabled):hover { opacity: 0.9; background: var(--hover-bg, #8881); }
  .lr-group { margin: 6px 0 10px; animation: lr-in 0.18s ease both; }
  @keyframes lr-in { from { opacity: 0; transform: translateY(2px); } to { opacity: 1; transform: none; } }
  .lr-file {
    background: none; border: none; text-align: left; cursor: pointer; padding: 2px 4px;
    border-radius: 4px; color: inherit; font-size: 1em; font-weight: 600; opacity: 0.85;
  }
  .lr-file:hover { background: var(--hover-bg, #8881); text-decoration: underline; }
  .lr-carrier { padding: 2px 0 2px 12px; }
  .lr-crumb {
    display: block; background: none; border: none; text-align: left; cursor: pointer;
    color: inherit; opacity: 0.45; font-size: 0.82em; padding: 0 4px 2px; font-family: inherit;
  }
  .lr-crumb:hover { opacity: 0.8; text-decoration: underline; }
</style>
