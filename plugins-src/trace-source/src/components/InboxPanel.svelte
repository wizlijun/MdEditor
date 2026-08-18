<!-- InboxPanel.svelte — every trace report in the report directory, newest
     first. Same geometry and grammar as idea-spark's inbox (a 240px column
     that *squeezes* the editor, hidden by default, toggled from the action
     bar), but a much simpler contract: a report is an agent product (✦) that
     either exists or doesn't — no drafts, no rename, no per-row status.

     A row is a button: clicking it opens the report in the MAIN editor (this
     window's editor is the delegation composer, not a reader). Right-click
     (or the keyboard's menu key) opens the row menu: open / delete.

     The row label is the report's frontmatter `title`, read by `listReports`
     (capped, tolerant — a null title falls back to the file name); the age
     comes from the timestamp the file is named with.

     Reactivity discipline: no `$effect` anywhere; the clock tick lives in
     `onMount` (MEMORY feedback_svelte_effect_untrack). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte'
  import Icon from './Icon.svelte'
  import { createdFromName, relativeAge, type ReportEntry } from '../lib/inbox'
  import { locale, t } from '../lib/strings'

  const {
    reports,
    listFailed,
    onopen,
    ondelete,
    onpreviewdelete,
    ontoggle,
  }: {
    reports: ReportEntry[]
    /** The directory listing itself failed — say so instead of "no reports". */
    listFailed: boolean
    /** Opens the report in the main editor. */
    onopen: (name: string) => void
    /** Deletes the report and its materials (App owns the IO). */
    ondelete: (name: string) => void | Promise<void>
    /** Names every file a delete would take, for the confirm dialog. */
    onpreviewdelete: (name: string) => Promise<string[]>
    /** The header's fold-away button — same `toggle` the action bar uses. */
    ontoggle: () => void
  } = $props()

  let menu = $state<{ name: string; x: number; y: number } | null>(null)
  let confirm = $state<{ name: string; lines: string[] } | null>(null)
  /** The row button the menu was opened from — focus goes back to it on close. */
  let opener: HTMLElement | null = null
  /** Re-read once a minute so "3 minutes ago" doesn't freeze. */
  let now = $state(new Date())

  const rtf = $derived(new Intl.RelativeTimeFormat(locale(), { numeric: 'auto' }))

  function label(r: ReportEntry): string {
    return r.title ?? r.name
  }

  function ageLabel(name: string): string {
    const created = createdFromName(name)
    if (!created) return ''
    const { value, unit } = relativeAge(created, now)
    return rtf.format(value, unit)
  }

  function openMenu(e: MouseEvent, name: string): void {
    e.preventDefault()
    opener = e.currentTarget as HTMLElement
    const fromKeyboard = e.clientX === 0 && e.clientY === 0
    const rect = opener.getBoundingClientRect()
    menu = {
      name,
      x: fromKeyboard ? rect.left + 8 : e.clientX,
      y: fromKeyboard ? rect.bottom : e.clientY,
    }
  }

  function closeMenu(): void {
    menu = null
    opener?.focus()
    opener = null
  }

  function itemsFor(name: string): MenuItem[] {
    return [
      { label: t('menuOpenReport'), icon: 'open-report', onselect: () => onopen(name) },
      { label: t('menuDelete'), icon: 'delete', danger: true, separated: true, onselect: () => void askDelete(name) },
    ]
  }

  async function askDelete(name: string): Promise<void> {
    confirm = { name, lines: await onpreviewdelete(name) }
  }

  async function confirmDelete(): Promise<void> {
    const name = confirm?.name
    confirm = null
    if (name !== undefined) await ondelete(name)
  }

  onMount(() => {
    const id = setInterval(() => (now = new Date()), 60_000)
    return () => clearInterval(id)
  })
</script>

<aside class="inbox" aria-label={t('inbox')}>
  <header>
    <button
      type="button"
      class="hidebtn"
      title={t('hideInbox')}
      aria-label={t('hideInbox')}
      onclick={ontoggle}
    >
      <Icon name="collapse" size={14} />
    </button>
    <h2>{t('inbox')}</h2>
  </header>
  {#if listFailed}
    <p class="unavailable">{t('listUnavailable')}</p>
  {:else if reports.length === 0}
    <p class="empty">{t('inboxEmpty')}</p>
  {/if}
  <ul>
    {#each reports as r (r.name)}
      <li>
        <button
          class="row"
          type="button"
          onclick={() => onopen(r.name)}
          oncontextmenu={(e) => openMenu(e, r.name)}
          title={r.name}
        >
          <span class="name">{label(r)}</span>
          <span class="age">{ageLabel(r.name)}</span>
        </button>
      </li>
    {/each}
  </ul>
</aside>

<!-- `{#key}`: the menu measures and positions itself once, when it mounts. -->
{#if menu}
  {#key menu}
    <ContextMenu
      x={menu.x}
      y={menu.y}
      label={label(reports.find((r) => r.name === menu!.name) ?? { name: menu.name, title: null })}
      items={itemsFor(menu.name)}
      onclose={closeMenu}
    />
  {/key}
{/if}

{#if confirm}
  <ConfirmDialog
    title={t('confirmDeleteTitle')}
    body={t('confirmDeleteBody')}
    lines={confirm.lines}
    confirmLabel={t('confirmDelete')}
    cancelLabel={t('cancel')}
    onconfirm={confirmDelete}
    oncancel={() => (confirm = null)}
  />
{/if}

<style>
  .inbox {
    width: 240px;
    flex: 0 0 240px;
    box-sizing: border-box;
    padding: 0.75rem;
    border-left: 1px solid var(--line, #e5e7eb);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    margin: 0 0 0.6rem -3px;
  }
  h2 {
    margin: 0;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.6;
  }
  .hidebtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
  }
  .hidebtn:hover {
    background: color-mix(in srgb, currentColor 10%, transparent);
    opacity: 1;
  }
  .unavailable {
    margin: 0 0 0.5rem;
    padding: 0.35rem 0.45rem;
    border-radius: 6px;
    background: color-mix(in srgb, #dc2626 12%, transparent);
    font-size: 0.75rem;
    line-height: 1.4;
  }
  .empty {
    margin: 0;
    font-size: 0.78rem;
    opacity: 0.55;
  }
  ul { list-style: none; margin: 0; padding: 0; }
  li {
    display: flex;
    flex-direction: column;
    border-radius: 6px;
    margin-bottom: 2px;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.35rem;
    width: 100%;
    padding: 0.4rem 0.45rem;
    background: none;
    border: 0;
    border-radius: 6px;
    color: inherit;
    /* button does NOT inherit font-size/family — MEMORY
       reference_button_no_inherit_font. */
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
  }
  .row:hover { background: color-mix(in srgb, currentColor 10%, transparent); }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .age {
    flex: 0 0 auto;
    font-size: 0.68rem;
    opacity: 0.5;
    white-space: nowrap;
  }
</style>
