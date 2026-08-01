<script lang="ts">
  import { bridge, vaultInfo, vaultList, vaultExists, openInEditor, toast } from './lib/bridge'
  import {
    buildIndex, buildDayIndex, parseDiaryName, parseDailyNoteName,
    WEEKLY_DIR, DIARY_DIR, DAILYNOTE_DIR, type ReviewIndex,
  } from './lib/scan'
  import { mondayOf } from './lib/isoweek'
  import { setLocale, t } from './lib/strings'
  import YearCalendar from './lib/components/YearCalendar.svelte'

  setLocale(bridge().locale)

  let reviewIndex = $state<ReviewIndex>({ byYear: new Map(), years: [] })
  let diaryIndex = $state<Map<string, string>>(new Map())
  let diaryYears = $state<Set<number>>(new Set())
  let noteYears = $state<Set<number>>(new Set())
  let noteByYear = $state<Map<number, Map<string, string>>>(new Map())
  let selectedYear = $state<number>(new Date().getFullYear())
  let loading = $state(true)
  let noVault = $state(false)
  let vaultRoot: string | null = null

  const now = new Date()
  const todayMondayMs = mondayOf(now).getTime()
  const currentYear = now.getFullYear()

  const reviewWeeks = $derived(reviewIndex.byYear.get(selectedYear))
  const noteIndex = $derived(noteByYear.get(selectedYear) ?? new Map<string, string>())
  const years = $derived.by(() => {
    const s = new Set<number>(reviewIndex.years)
    for (const y of diaryYears) s.add(y)
    for (const y of noteYears) s.add(y)
    return [...s].sort((a, b) => a - b)
  })

  function hasYear(y: number): boolean {
    return reviewIndex.byYear.has(y) || diaryYears.has(y) || noteYears.has(y)
  }
  function pickDefaultYear(): number {
    if (hasYear(currentYear)) return currentYear
    const ys = years
    return ys.length ? ys[ys.length - 1] : currentYear
  }
  function yearsOf(map: Map<string, string>): Set<number> {
    const s = new Set<number>()
    for (const k of map.keys()) s.add(Number(k.slice(0, 4)))
    return s
  }
  function scrollToToday() {
    requestAnimationFrame(() =>
      document.getElementById('wr-today')?.scrollIntoView({ block: 'center', behavior: 'smooth' }),
    )
  }

  async function ensureYear(year: number) {
    if (!vaultRoot || noteByYear.has(year)) return
    const { loadCache, saveCache } = await import('./lib/cache')
    const dir = `${DAILYNOTE_DIR}/${year}`
    const cached = loadCache(vaultRoot, `dailynote:${year}`)
    if (cached) {
      const m = new Map(noteByYear)
      m.set(year, buildDayIndex(cached.map((name) => ({ name, is_dir: false })), dir, parseDailyNoteName))
      noteByYear = m
    }
    const exists = await vaultExists(dir)
    const entries = exists ? await vaultList(dir) : []
    const m2 = new Map(noteByYear)
    m2.set(year, buildDayIndex(entries, dir, parseDailyNoteName))
    noteByYear = m2
    saveCache(vaultRoot, `dailynote:${year}`, entries.map((e) => e.name))
  }

  async function scan(force = false) {
    try {
      const info = await vaultInfo()
      vaultRoot = info.root
      if (!vaultRoot) { noVault = true; loading = false; return }
      const { loadCache, saveCache } = await import('./lib/cache')

      if (!force) {
        const rc = loadCache(vaultRoot, 'weekly-review')
        if (rc) reviewIndex = buildIndex(rc.map((name) => ({ name, is_dir: false })))
        const dc = loadCache(vaultRoot, 'diary')
        if (dc) { diaryIndex = buildDayIndex(dc.map((name) => ({ name, is_dir: false })), DIARY_DIR, parseDiaryName); diaryYears = yearsOf(diaryIndex) }
        selectedYear = pickDefaultYear()
      }

      const rExists = await vaultExists(WEEKLY_DIR)
      const rEntries = rExists ? await vaultList(WEEKLY_DIR) : []
      reviewIndex = buildIndex(rEntries)
      saveCache(vaultRoot, 'weekly-review', rEntries.map((e) => e.name))

      const dExists = await vaultExists(DIARY_DIR)
      const dEntries = dExists ? await vaultList(DIARY_DIR) : []
      diaryIndex = buildDayIndex(dEntries, DIARY_DIR, parseDiaryName)
      diaryYears = yearsOf(diaryIndex)
      saveCache(vaultRoot, 'diary', dEntries.map((e) => e.name))

      const nExists = await vaultExists(DAILYNOTE_DIR)
      const nDirs = nExists ? await vaultList(DAILYNOTE_DIR) : []
      noteYears = new Set(nDirs.filter((e) => e.is_dir && /^\d{4}$/.test(e.name)).map((e) => Number(e.name)))

      if (!hasYear(selectedYear)) selectedYear = pickDefaultYear()
      await ensureYear(selectedYear)
      if (selectedYear === currentYear) scrollToToday()
    } catch (e) {
      await toast('error', t('title'), String(e))
    } finally {
      loading = false
    }
  }

  function selectYear(y: number) { selectedYear = y; ensureYear(y) }
  function stepYear(delta: number) { selectYear(selectedYear + delta) }
  function goThisWeek() { selectedYear = currentYear; ensureYear(currentYear); scrollToToday() }
  async function onOpen(path: string) {
    try { await openInEditor(path) } catch (e) { await toast('error', t('title'), String(e)) }
  }

  scan()
</script>

<div class="app">
  <header class="head">
    <div class="yearart">{selectedYear}</div>
    <div class="subtitle">{t('title')}</div>
    <div class="spacer"></div>
    <div class="toolbar">
      <button class="arrow" onclick={() => stepYear(-1)} aria-label={t('nav.prevYear')}>‹</button>
      <div class="years">
        {#each years as y}
          <button class="ychip" class:active={y === selectedYear} onclick={() => selectYear(y)}>{y}</button>
        {/each}
      </div>
      <button class="arrow" onclick={() => stepYear(1)} aria-label={t('nav.nextYear')}>›</button>
      <button class="tbtn accent" onclick={goThisWeek}>◎ {t('thisWeek')}</button>
      <button class="tbtn" onclick={() => scan(true)}>↻ {t('rebuild')}</button>
    </div>
  </header>

  {#if noVault}
    <div class="empty">{t('empty.noVault')}</div>
  {:else if !loading && years.length === 0}
    <div class="empty">{t('empty.noData')}</div>
  {:else}
    <YearCalendar year={selectedYear} weeks={reviewWeeks} {diaryIndex} {noteIndex} {todayMondayMs} {onOpen} />
  {/if}
</div>

<style>
  :global(:root) {
    color-scheme: light dark;
    --bg: #fff; --fg: #22252a; --muted: #9aa0a8; --line: #e6e8ec; --wm: #f0f1f4;
    --chip-bg: #f2f3f5; --chip-active: #2f6feb;
    --accent: #2f6feb; --accent-fg: #fff; --link: #2f6feb; --note: #e8a13a;
    --past: #f1f2f4; --past-day: #aab0b8;
    --future-line: #eaecef; --future-day: #c6ccd4;
    --today-ring: #ff9500; --weekend: #e0605a; --yearart: #d21f2b;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #191b1f; --fg: #e7e9ec; --muted: #7b8189; --line: #2a2d33; --wm: #212429;
      --chip-bg: #2a2d33; --chip-active: #3b82f6;
      --accent: #4b8bff; --accent-fg: #fff; --link: #6aa5ff; --note: #f0b657;
      --past: #212429; --past-day: #767c85;
      --future-line: #282b31; --future-day: #565c65;
      --today-ring: #ffa726; --weekend: #e0736d; --yearart: #ff5a63;
    }
  }
  :global(html), :global(body) { height: 100%; }
  :global(body) { margin: 0; background: var(--bg); color: var(--fg); font: 12px/1.3 -apple-system, 'SF Pro Text', 'PingFang SC', system-ui, sans-serif; }
  .app { height: 100vh; overflow: hidden; display: flex; flex-direction: column; }
  .head { flex: 0 0 auto; display: flex; align-items: center; gap: 16px; padding: 10px 20px 6px; }
  .yearart { font-family: 'Snell Roundhand', 'Zapfino', 'Brush Script MT', cursive; font-weight: 700; font-style: italic; font-size: 46px; line-height: 1; color: var(--yearart); }
  .subtitle { font-size: 12px; color: var(--muted); font-weight: 600; letter-spacing: 3px; }
  .spacer { flex: 1; }
  .toolbar { display: flex; align-items: center; gap: 7px; }
  .arrow { width: 26px; height: 26px; border-radius: 7px; border: 1px solid var(--line); background: var(--chip-bg); color: var(--fg); font-size: 14px; cursor: pointer; }
  .years { display: flex; gap: 5px; }
  .ychip { padding: 4px 10px; border-radius: 20px; background: var(--chip-bg); color: var(--fg); font-weight: 600; font-size: 12px; cursor: pointer; border: none; }
  .ychip.active { background: var(--chip-active); color: #fff; }
  .tbtn { display: flex; align-items: center; gap: 5px; padding: 5px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--chip-bg); color: var(--fg); font-weight: 600; font-size: 12px; cursor: pointer; }
  .tbtn.accent { border-color: var(--accent); color: var(--accent); background: transparent; }
  .empty { flex: 1 1 auto; display: flex; align-items: center; justify-content: center; padding: 40px; text-align: center; color: var(--muted); font-size: 13px; }
</style>
