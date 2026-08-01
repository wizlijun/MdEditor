<script lang="ts">
  import { buildMonthRows } from '../isoweek'
  import { t } from '../strings'
  import WeekRow from './WeekRow.svelte'

  interface Props {
    year: number
    month0: number
    weeks: Map<number, string> | undefined
    diaryIndex: Map<string, string>
    noteIndex: Map<string, string>
    todayMondayMs: number
    onOpen: (path: string) => void
  }
  let { year, month0, weeks, diaryIndex, noteIndex, todayMondayMs, onOpen }: Props = $props()

  const DOW = [t('dow.mon'), t('dow.tue'), t('dow.wed'), t('dow.thu'), t('dow.fri'), t('dow.sat'), t('dow.sun')]
  const rows = $derived(buildMonthRows(year, month0))
</script>

<div class="month">
  <div class="wm">{month0 + 1}</div>
  <div class="mtitle">{month0 + 1}{t('month.suffix')}</div>
  <div class="dow">
    {#each DOW as d, i}<span class:we={i >= 5}>{d}</span>{/each}
  </div>
  <div class="weeks">
    {#each rows as row}
      <WeekRow
        {row}
        {year}
        {month0}
        reviewPath={weeks?.get(row.week) ?? null}
        {diaryIndex}
        {noteIndex}
        isToday={row.monday.getTime() === todayMondayMs}
        isFuture={row.monday.getTime() > todayMondayMs}
        {onOpen}
      />
    {/each}
  </div>
</div>

<style>
  .month { position: relative; display: flex; flex-direction: column; min-height: 0; }
  .wm { position: absolute; right: 0; top: 8px; font-size: 44px; font-weight: 800; color: var(--wm); z-index: 0; pointer-events: none; letter-spacing: -2px; }
  .mtitle { font-weight: 700; font-size: 12px; margin: 0 0 2px 2px; position: relative; z-index: 1; }
  .dow { display: grid; grid-template-columns: repeat(7, 1fr); position: relative; z-index: 1; }
  .dow span { text-align: center; font-size: 9px; color: var(--muted); font-weight: 600; }
  .dow span.we { color: var(--weekend); }
  .weeks { position: relative; z-index: 1; display: flex; flex-direction: column; gap: 2px; flex: 1 1 auto; min-height: 0; }
</style>
