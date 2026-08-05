<!-- InboxPanel.svelte — every idea in the idea directory, newest first
     (replaces the old HistoryList). A 240px column that *squeezes* the editor
     rather than covering it, hidden by default and toggled from the action bar.

     A row is a button: clicking it loads that idea into the editor. Right-click
     (or the keyboard's own menu key / Shift+F10, which fire the same
     `contextmenu` event) opens ContextMenu with delegate / open in the main
     editor / rename / delete.

     ## What a row says, and where it comes from

     The file name is no longer a label: names are creation timestamps
     (`2026-08-04-1942-idea.md`), so a column of them tells you nothing about
     what any of the ideas are. The label is therefore read out of the document
     itself — `rowTitle` → the body's H1, its first non-empty line, or the file
     name as a last resort.

     That means an extra file read per row, which is why the reads are LAZY and
     CACHED: an `IntersectionObserver` asks `ensureTitle(name)` only for rows
     that actually scroll into view, and the store keeps what comes back
     (`store.titles`). An inbox holding several hundred ideas therefore costs a
     screenful of reads to draw, not several hundred — and scrolling to the
     bottom pays for what it passes, once.

     The cache is kept honest from three directions, none of which re-reads a
     file it doesn't already have in hand: this window's own writes update it
     (`saveIdea` / `loadIdea` / `renameIdea` / `deleteIdea`), and regaining
     window focus drops it wholesale — that is the moment the user plausibly
     comes back from editing an idea in the main editor (which this very panel
     can open), or from an agent or a sync having rewritten one. Only the rows
     on screen are re-read then; the rest re-hydrate as they scroll past.

     Status comes from `statusOf` (proof file > active run > last failure >
     draft); the panel never stores a status of its own.

     Reactivity discipline: no `$effect` anywhere. The observer and the clock
     tick are set up in `onMount`, and menu/dialog/rename are plain `$state`
     driven by events — an effect that synchronously calls a store function
     which reads and writes `$state` self-invalidates into a loop that freezes
     the window (MEMORY feedback_svelte_effect_untrack). -->
<script lang="ts">
  import { onMount } from 'svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte'
  import Icon from './Icon.svelte'
  import { proofPathFor } from '../lib/naming'
  // The status → badge tables live in `status.ts` (pure TS, no DOM) so that
  // `STATUS_MARK.done`'s `✦` — a product convention, not a glyph choice — is
  // pinned by `status.test.ts`. A component-private constant is unreachable
  // from the test suite, which is exactly what it must not be.
  import { STATUS_KEY, STATUS_MARK } from '../lib/status'
  import {
    createdFromName,
    ensureTitle,
    filesToDelete,
    invalidateTitles,
    openIdea,
    openResult,
    relativeAge,
    relPath,
    runInFlight,
    state as store,
    statusOf,
    titleOf,
  } from '../lib/store.svelte'
  import { locale, t } from '../lib/strings'

  const {
    onselect,
    ondelete,
    onrename,
    ondelegate,
  }: {
    onselect: (name: string) => void
    /** Deletes the idea. The App wrapper flushes the autosave first. */
    ondelete: (name: string) => void | Promise<void>
    /** Renames it; false means the name was refused (the row stays in edit mode). */
    onrename: (from: string, raw: string) => Promise<boolean>
    /** Hands the idea to the agent. Same App-level chain as the action bar's
     *  own button — flush, run, register the run — just aimed at a row that
     *  isn't necessarily the open document. */
    ondelegate: (name: string) => void
  } = $props()

  let menu = $state<{ name: string; x: number; y: number } | null>(null)
  let confirm = $state<{ name: string; lines: string[] } | null>(null)
  let renaming = $state<string | null>(null)
  let renameValue = $state('')
  /** The row button the menu was opened from — focus goes back to it on close. */
  let opener: HTMLElement | null = null
  /** Re-read once a minute so "3 minutes ago" doesn't freeze at whatever it
   *  said when the panel opened. */
  let now = $state(new Date())

  const rtf = $derived(new Intl.RelativeTimeFormat(locale(), { numeric: 'auto' }))

  function ageLabel(name: string): string {
    const created = createdFromName(name)
    if (!created) return '' // renamed out of the timestamp convention: say nothing
    const { value, unit } = relativeAge(created, now)
    return rtf.format(value, unit)
  }

  // ── lazy row titles ───────────────────────────────────────────────────────

  /** Rows currently on screen — what has to be re-read when the cache is
   *  dropped (see the `focus` listener). Plain Set: nothing renders it. */
  const visible = new Set<string>()

  // Built at component init, NOT in `onMount`: Svelte runs an element's actions
  // as it creates the element, which is before `onMount` — an observer created
  // there would miss the first screenful of rows entirely.
  const observer =
    typeof IntersectionObserver === 'undefined'
      ? null
      : new IntersectionObserver(
          (entries) => {
            for (const entry of entries) {
              const name = (entry.target as HTMLElement).dataset.idea
              if (!name) continue
              if (!entry.isIntersecting) {
                visible.delete(name)
                continue
              }
              visible.add(name)
              void ensureTitle(name)
            }
          },
          // No explicit root: the viewport, clipped by this panel's own
          // `overflow-y: auto` (which the intersection accounts for). The
          // margin reads a little beyond the fold so a slow scroll doesn't
          // show a screen of file names before the titles land.
          { rootMargin: '200px' },
        )

  /** `use:hydrate` — read this row's title when (and only when) it is seen. */
  function hydrate(node: HTMLElement, name: string) {
    if (!observer) {
      // No IntersectionObserver (very old webview): correctness beats
      // frugality — read every row rather than showing bare file names.
      visible.add(name)
      void ensureTitle(name)
      return {
        destroy() {
          visible.delete(name)
        },
      }
    }
    observer.observe(node)
    return {
      destroy() {
        visible.delete(name)
        observer.unobserve(node)
      },
    }
  }

  // ── the context menu ──────────────────────────────────────────────────────

  function openMenu(e: MouseEvent, name: string): void {
    e.preventDefault()
    opener = e.currentTarget as HTMLElement
    // A keyboard-raised context menu (menu key / Shift+F10) reports 0,0 — a
    // menu in the window's top-left corner would be nowhere near the row it
    // belongs to, so anchor it to the row itself.
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
    // Focus came from a row; hand it back rather than dropping it on <body>,
    // where the next arrow key would do nothing at all.
    opener?.focus()
    opener = null
  }

  function itemsFor(name: string): MenuItem[] {
    const hasProof = store.files.includes(proofPathFor(relPath(store, name)))
    // Greyed out while ANY idea has a run in flight — claude-agent serializes
    // the whole `idea-proof` task (see `runInFlight`), and this must be the
    // very same predicate the action bar's button uses. Keying it on
    // `statusOf(name)` instead would disagree with the button on two counts:
    // it would allow a second idea to be delegated (a run that looks started,
    // writes no record, and reads back as `lost` two seconds later), and it
    // would report `done` — hence enabled — for an idea whose own re-run is
    // live, since `deriveStatus` ranks a `.proof.md` above a running run.
    const busy = runInFlight(store)
    return [
      {
        label: t('menuDelegate'),
        icon: 'delegate',
        disabled: busy,
        title: busy ? t('delegateBusy') : t('menuDelegate'),
        onselect: () => ondelegate(name),
      },
      // With a proof document there are two things to open, so both are named
      // outright instead of hiding one behind a submenu (design §5 asks for a
      // second level; two flat rows say the same thing and stay reachable with
      // the arrow keys alone). The icons carry that distinction too: a plain
      // "open elsewhere" box for the idea the user wrote, a document with a
      // spark for the one the agent produced.
      ...(hasProof
        ? [
            { label: t('menuOpenIdea'), icon: 'open-idea' as const, onselect: () => void openIdea(name) },
            { label: t('menuOpenProof'), icon: 'open-proof' as const, onselect: () => void openResult(name) },
          ]
        : [{ label: t('menuOpenInMain'), icon: 'open-idea' as const, onselect: () => void openIdea(name) }]),
      { label: t('menuRename'), icon: 'rename', onselect: () => startRename(name) },
      { label: t('menuDelete'), icon: 'delete', danger: true, separated: true, onselect: () => askDelete(name) },
    ]
  }

  // ── rename (in place, in the row) ─────────────────────────────────────────

  function startRename(name: string): void {
    renaming = name
    // The file name, not the row's title: the user is renaming a file, and the
    // extension is part of what they may want to keep.
    renameValue = name
  }

  /**
   * `use:takeFocus` — focus the rename field and pre-select the base name.
   *
   * Not the `autofocus` attribute: the field appears in response to a menu
   * choice, and `choose` has just handed focus back to the row it replaced, so
   * the focus has to be taken explicitly rather than hoped for. Leaving `.md`
   * out of the selection is the Finder/Explorer convention — typing replaces
   * the name and keeps the extension.
   */
  function takeFocus(node: HTMLInputElement) {
    node.focus()
    const end = node.value.endsWith('.md') ? node.value.length - 3 : node.value.length
    node.setSelectionRange(0, end)
  }

  async function commitRename(): Promise<void> {
    const from = renaming
    if (from === null) return
    const ok = await onrename(from, renameValue)
    // A refused name keeps the field open (the store has already said why via a
    // toast) so the user can correct it instead of retyping from scratch.
    if (ok) renaming = null
  }

  function onRenameKey(e: KeyboardEvent): void {
    if (e.key === 'Enter') {
      e.preventDefault()
      void commitRename()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      renaming = null
    }
  }

  // ── delete ────────────────────────────────────────────────────────────────

  function askDelete(name: string): void {
    confirm = { name, lines: filesToDelete(store, name) }
  }

  async function confirmDelete(): Promise<void> {
    const name = confirm?.name
    confirm = null
    if (name === undefined) return
    if (renaming === name) renaming = null
    await ondelete(name)
  }

  onMount(() => {
    const id = setInterval(() => (now = new Date()), 60_000)

    // Coming back to this window is the one moment we can be fairly sure the
    // ideas may have been edited behind our back — by the main editor (which
    // this panel's own context menu opens), by an agent, by a vault sync. The
    // cached labels are dropped and the rows on screen re-read; everything
    // below the fold re-hydrates as it scrolls past, as it always does.
    const onFocus = () => {
      invalidateTitles()
      for (const name of visible) void ensureTitle(name)
    }
    window.addEventListener('focus', onFocus)

    return () => {
      clearInterval(id)
      window.removeEventListener('focus', onFocus)
      observer?.disconnect()
    }
  })
</script>

<aside class="inbox" aria-label={t('inbox')}>
  <h2>{t('inbox')}</h2>
  {#if store.listFailed}
    <!-- An empty list here would otherwise read as "you have no ideas yet",
         which is a lie when the directory simply couldn't be read. -->
    <p class="unavailable">{t('historyUnavailable')}</p>
  {:else if store.docs.length === 0}
    <p class="empty">{t('inboxEmpty')}</p>
  {/if}
  <ul>
    {#each store.docs as name (name)}
      {@const status = statusOf(store, name)}
      {@const mark = STATUS_MARK[status]}
      <li class:current={name === store.current}>
        {#if renaming === name}
          <input
            class="rename"
            type="text"
            bind:value={renameValue}
            aria-label={t('menuRename')}
            title={t('renameHint')}
            spellcheck="false"
            autocomplete="off"
            use:takeFocus
            onkeydown={onRenameKey}
            onblur={() => (renaming = null)}
          />
        {:else}
          <button
            class="row"
            type="button"
            onclick={() => onselect(name)}
            oncontextmenu={(e) => openMenu(e, name)}
            title={name}
            use:hydrate={name}
            data-idea={name}
          >
            <span class="name">{titleOf(store, name)}</span>
            {#if mark}
              <!-- `role="img"` is what makes the `aria-label` count: an icon
                   mark has no text of its own to name it (the SVG is
                   `aria-hidden`), and a bare `<span aria-label>` with no role
                   is ignored by assistive tech. -->
              <span
                class="mark {status}"
                role="img"
                title={t(STATUS_KEY[status])}
                aria-label={t(STATUS_KEY[status])}
              >
                {#if mark.kind === 'icon'}
                  <Icon name={mark.icon} size={12} />
                {:else}
                  {mark.text}
                {/if}
              </span>
            {/if}
            <span class="age">{ageLabel(name)}</span>
          </button>
        {/if}
      </li>
    {/each}
  </ul>
</aside>

<!-- `{#key}`: the menu measures and positions itself once, when it mounts. Two
     right-clicks in a row must therefore build two menus — updating the props
     of a live one would leave it drawn at the first row's coordinates. -->
{#if menu}
  {#key menu}
    <ContextMenu
      x={menu.x}
      y={menu.y}
      label={titleOf(store, menu.name)}
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
  h2 {
    margin: 0 0 0.6rem;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.6;
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
  li.current { background: color-mix(in srgb, currentColor 8%, transparent); }
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
    /* button does NOT inherit font-size/family — see MEMORY
       reference_button_no_inherit_font; both must be stated explicitly. */
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
  /* The mark is a 12px SVG for `running`/`failed` and the literal `✦` for
     `done`. The row is baseline-aligned (which is right for its text), so the
     mark opts out with `align-self: center` — an `<svg display:block>` inside
     an inline span puts its bottom edge on the baseline and would ride a
     couple of pixels low next to the `✦`. */
  .mark {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    align-self: center;
    font-size: 0.75rem;
  }
  .mark.done { color: #16a34a; }
  .mark.failed { color: #dc2626; }
  .age {
    flex: 0 0 auto;
    font-size: 0.68rem;
    opacity: 0.5;
    white-space: nowrap;
  }
  .rename {
    width: 100%;
    box-sizing: border-box;
    padding: 0.35rem 0.4rem;
    border: 1px solid var(--accent, #2563eb);
    border-radius: 6px;
    background: Field;
    color: FieldText;
    /* input does not inherit font-size/family either (MEMORY note). */
    font: inherit;
    font-size: 0.82rem;
  }
</style>
