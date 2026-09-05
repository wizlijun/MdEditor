<script lang="ts">
  import MonthGrid from './MonthGrid.svelte'
  import { t } from '../strings'

  interface Props {
    year: number
    weeks: Map<number, string> | undefined
    diaryIndex: Map<string, string>
    noteIndex: Map<string, string>
    todayMondayMs: number
    onOpen: (path: string) => void
  }
  let { year, weeks, diaryIndex, noteIndex, todayMondayMs, onOpen }: Props = $props()
  const months = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
</script>

<div class="cal">
  {#each months as m}
    <MonthGrid {year} month0={m} {weeks} {diaryIndex} {noteIndex} {todayMondayMs} {onOpen} />
  {/each}
</div>

<div class="legend">
  <div class="it"><span class="sw review"></span>{t('legend.review')}</div>
  <div class="it"><span class="sw today"></span>{t('legend.today')}</div>
  <div class="it"><span class="lk">12</span>{t('legend.diary')}</div>
  <div class="it"><svg class="star" viewBox="0 0 24 24"><path d="M12 .5 C12.8 7.2 16.8 11.2 23.5 12 C16.8 12.8 12.8 16.8 12 23.5 C11.2 16.8 7.2 12.8 .5 12 C7.2 11.2 11.2 7.2 12 .5Z"/></svg>{t('legend.note')}</div>
  <div class="it"><span class="sw past"></span>{t('legend.past')}</div>
</div>

<style>
  .cal { flex: 1 0 auto; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); grid-auto-rows: minmax(160px, 1fr); gap: 16px; padding: 6px 20px 8px; }
  .legend { flex: 0 0 auto; display: flex; gap: 14px; margin: 0 20px; padding: 8px 0 10px; border-top: 1px solid var(--line); font-size: 12px; color: var(--muted); flex-wrap: wrap; align-items: center; }
  .legend .it { display: flex; align-items: center; gap: 5px; }
  .sw { width: 20px; height: 13px; border-radius: 4px; }
  .sw.review { background: var(--accent); }
  .sw.today { background: var(--past); outline: 2px solid var(--today-ring); outline-offset: -2px; }
  .sw.past { background: var(--past); }
  .lk { color: var(--link); text-decoration: underline; font-weight: 800; }
  .star { width: 13px; height: 13px; fill: var(--note); }
  @media (max-width: 900px) { .cal { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 680px) { .cal { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
