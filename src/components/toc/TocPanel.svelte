<script lang="ts">
  import { tick } from 'svelte'
  import type { Tab } from '../../lib/tabs.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import { requestReveal } from '../../lib/outline/reveal.svelte'
  import { setSideVisible } from '../../lib/side-panel/registry.svelte'
  import { extractTocHeadings, type TocHeading } from '../../lib/toc/headings'
  import {
    beginTocLocationTracking,
    reportTocLocation,
    tocLocation,
  } from '../../lib/toc/location.svelte'
  import SideViewSwitcher from '../side-panel/SideViewSwitcher.svelte'

  let { tab }: { tab: Tab | null } = $props()

  let applicable = $derived(tab != null && tab.kind === 'markdown')
  let headings = $derived(applicable && tab ? extractTocHeadings(tab.currentContent) : [])
  let activeHeadingIndex = $derived(
    tab && tocLocation.trackedTabId === tab.id ? tocLocation.activeHeadingIndex : null,
  )
  let tocListEl: HTMLOListElement | undefined = $state()

  // SidePanel only mounts this component while TOC is the visible right view.
  // Its lifecycle is therefore the tracking gate for both editor modes.
  $effect(() => {
    if (!applicable || !tab) return
    return beginTocLocationTracking(tab.id)
  })

  $effect(() => {
    const headingIndex = activeHeadingIndex
    const list = tocListEl
    if (headingIndex == null || !list) return
    void tick().then(() => {
      const row = Array.from(list.querySelectorAll<HTMLButtonElement>('.toc-row'))
        .find((button) => Number(button.dataset.headingIndex) === headingIndex)
      if (row && typeof row.scrollIntoView === 'function') {
        row.scrollIntoView({ block: 'nearest' })
      }
    })
  })

  function jumpTo(heading: TocHeading) {
    if (!tab) return
    reportTocLocation(tab.id, heading.headingIndex)
    requestReveal(heading.line, heading.text, tab.filePath || null, {
      headingIndex: heading.headingIndex,
    })
  }
</script>

<div class="toc-content">
  <header>
    <button
      class="hbtn hide-button"
      title={t('toc.hide')}
      aria-label={t('toc.hide')}
      onclick={() => void setSideVisible('right', false)}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2" />
        <line x1="15" y1="3" x2="15" y2="21" />
        <polyline points="8 9 11 12 8 15" />
      </svg>
    </button>
    <SideViewSwitcher side="right" {tab} />
  </header>

  {#if !applicable}
    <div class="body"><p class="empty">{tab == null ? t('toc.noDocument') : t('toc.notApplicable')}</p></div>
  {:else if headings.length === 0}
    <div class="body"><p class="empty">{t('toc.empty')}</p></div>
  {:else}
    <nav class="body" aria-label={t('toc.title')}>
      <ol class="toc-list" bind:this={tocListEl}>
        {#each headings as heading (`${heading.headingIndex}:${heading.line}`)}
          <li>
            <button
              class="toc-row"
              class:top-level={heading.level === 1}
              class:current={activeHeadingIndex === heading.headingIndex}
              data-level={heading.level}
              data-heading-index={heading.headingIndex}
              style={`--toc-depth: ${heading.depth}`}
              aria-current={activeHeadingIndex === heading.headingIndex ? 'location' : undefined}
              title={t('toc.jumpTo', { line: heading.line, title: heading.text })}
              aria-label={t('toc.jumpTo', { line: heading.line, title: heading.text })}
              onclick={() => jumpTo(heading)}
            >
              <span class="marker" aria-hidden="true"></span>
              <span class="label">{heading.text}</span>
              <span class="line" aria-hidden="true">{heading.line}</span>
            </button>
          </li>
        {/each}
      </ol>
    </nav>
  {/if}
</div>

<style>
  .toc-content {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid var(--border-color, #3333);
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .hbtn {
    display: inline-flex; align-items: center; justify-content: center;
    border: 0; background: transparent; cursor: pointer;
    padding: 3px; border-radius: 4px; opacity: 0.7;
    color: inherit;
  }
  .hbtn svg { display: block; }
  .hbtn:hover { background: rgba(0, 0, 0, 0.08); opacity: 1; }
  .body { flex: 1; overflow-y: auto; padding: 8px; }
  .empty { opacity: 0.5; font-size: 12px; }
  .toc-list { list-style: none; margin: 0; padding: 0; }
  .toc-row {
    width: 100%;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 7px 6px calc(7px + var(--toc-depth) * 16px);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font: inherit;
    font-size: 13px;
    line-height: 1.35;
    text-align: left;
  }
  .toc-row:hover { background: rgba(0, 0, 0, 0.05); }
  .toc-row.current,
  .toc-row.current:hover {
    background: color-mix(in srgb, var(--accent-color, #4a80d4) 18%, transparent);
    color: var(--accent-color, #3266b1);
  }
  .toc-row.current .marker { opacity: 1; }
  .toc-row.current .label { font-weight: 650; }
  .toc-row.current .line { opacity: 0.64; }
  .toc-row:focus-visible {
    outline: 2px solid var(--accent-color, #4a80d4);
    outline-offset: -2px;
  }
  .marker {
    flex: 0 0 auto;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.38;
  }
  .top-level .marker { opacity: 0.72; }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .top-level .label { font-weight: 600; }
  .line {
    flex: 0 0 auto;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    opacity: 0;
  }
  .toc-row:hover .line,
  .toc-row:focus-visible .line { opacity: 0.42; }
  @media (prefers-color-scheme: dark) {
    .hbtn:hover,
    .toc-row:hover { background: rgba(255, 255, 255, 0.1); }
    .toc-row.current,
    .toc-row.current:hover {
      background: color-mix(in srgb, var(--accent-color, #72a7ff) 26%, transparent);
      color: var(--accent-color, #8db7ff);
    }
  }
</style>
