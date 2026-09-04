<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte'
  import type { Tab } from '../lib/tabs.svelte'
  import { setContent, activeTab, openFile } from '../lib/tabs.svelte'
  import { classifyLink, resolveWikilinkPath, restoreWikilinks, type LinkAction } from '../lib/link-open'
  import { buildFencedBlock, stripCodeFence } from '../lib/code-fence'
  import { toDisplayMarkdown } from '../lib/mdx/display'
  import { activeTheme } from '../lib/active-theme.svelte'
  import RichGutter from '../lib/mdblock-hover/rich-gutter.svelte'
  import {
    hoverStore,
    getDisplayYaml,
    loadHoverYaml,
    recomputeLiveYaml,
    isHoverActive,
  } from '../lib/mdblock-hover/hover-store.svelte'
  import { settings } from '../lib/settings.svelte'
  import { createImeGuard } from '../lib/ime'
  import { t } from '../lib/i18n/store.svelte'
  import { answersStore, loadAnswersFor } from '../lib/note-anno/answers-store.svelte'
  import '../lib/styles/attachment.css'
  import ImageToolbar from '../lib/image-toolbar/ImageToolbar.svelte'
  import { saveClipboardResource, isAttachmentUrl, isImageExt, isAttachmentExt } from '../lib/paste-resources'
  import { insertImageAtCursor, insertAttachmentLink, insertImageAtPos } from '../lib/attachment-insert'
  import { isVideoUrl, fetchVideoInfo } from '../lib/video-links'
  import type { EditorView } from 'prosemirror-view'
  import SlashMenu from '../lib/slash-menu/SlashMenu.svelte'
  import { getSlashItems, filterSlashItems, type SlashItem } from '../lib/slash-menu/slash-items'
  import EditorContextMenu, { type EditorActions } from '../lib/context-menu/EditorContextMenu.svelte'
  import { createRichActions } from '../lib/context-menu/rich-actions'
  import { noteUi, readThemeStyle } from '../lib/note-anno/note-ui.svelte'
  import { openEditForMark, openEditForAnchor, insertNoteRich } from '../lib/note-anno/note-commands'
  import NotePopover from '../lib/note-anno/NotePopover.svelte'
  import NoteEditPopup from '../lib/note-anno/NoteEditPopup.svelte'
  import { applySelectAll, handleSelectAllKeydown } from '../lib/editor-select-all'
  import { setBlockType, wrapIn } from 'prosemirror-commands'
  import { wrapInList } from 'prosemirror-schema-list'
  import LinkedReferences from './outline/LinkedReferences.svelte'
  import { pageNameOf } from '../lib/outline/backlinks'
  import { ensureIndex } from '../lib/outline/backlinks-io.svelte'
  import { outlineGate } from '../lib/outline/gate.svelte'

  // Reactive store of the currently active theme id, set by the theme-init
  // block in App.svelte. Default is 'default'.
  const activeThemeId = $derived(activeTheme.id)

  // NOTE: @moraya/core (ProseMirror + plugins, multi-MB) is dynamically imported
  // inside onMount so it never loads when the user only uses source mode.
  type EditorInstance = {
    view: unknown
    getMarkdown(): string
    setContent(md: string): void
    destroy(): void
  }

  let {
    tab,
    onFlush,
    wrapAsCodeBlock,
    readOnly = false,
  }: {
    tab: Tab
    onFlush?: (md: string) => void
    /**
     * Render-only mode: the view is mounted non-editable and nothing is ever
     * pushed back into the tab. Used by mdx, whose JSX/import lines would not
     * survive a markdown round-trip — see `lib/mdx/display.ts`.
     */
    readOnly?: boolean
    /**
     * If defined, the editor is mounted with content wrapped in a fenced block
     * (` ```<lang>...``` `) and `onChange` / `onDestroy` strip the fence before
     * propagating raw content back. Used for code-kind tabs.
     */
    wrapAsCodeBlock?: string
  } = $props()

  // Wiki page name for the inline backlink recall section (rendered below the
  // document body). Gated on the outline-notes feature; skipped for code-kind
  // tabs and untitled docs.
  const backlinkPage = $derived(
    outlineGate.enabled && !wrapAsCodeBlock && tab.filePath ? pageNameOf(tab.filePath) : null,
  )

  // The backlink index is normally built lazily by the outline panel. When a
  // wiki page is shown in the editor without ever opening that panel, build it
  // here too. `ensureIndex` is idempotent (early-returns once the vault root is
  // indexed). `untrack` keeps its internal store reads/writes out of this
  // effect's dependencies — otherwise the bump()/assignment inside would
  // self-invalidate the effect (see the $effect-untrack lesson).
  $effect(() => {
    const fp = tab.filePath
    if (!backlinkPage || !fp) return
    untrack(() => void ensureIndex(fp))
  })

  let host: HTMLDivElement | undefined = $state()
  let editor: EditorInstance | null = null
  let status = $state<'mounting' | 'mounted' | 'error'>('mounting')
  let errorMsg = $state<string | null>(null)

  function focusRichEditorAtEnd(view: any, delayMs = 60) {
    void getPmState().then(({ Selection }) => {
      setTimeout(() => {
        try {
          const doc = view.state.doc
          // Selection.atEnd finds the nearest valid text position; a raw
          // TextSelection at doc.content.size throws on a doc with no inline
          // content (e.g. an empty doc).
          view.dispatch(view.state.tr.setSelection(Selection.atEnd(doc)))
          view.focus()
        } catch { /* ignore */ }
      }, delayMs)
    })
  }

  /** Clicking the blank area below the document puts the caret at the end
   *  instead of doing nothing — the whole pane behaves as writing surface. */
  function onScrollMouseDown(e: MouseEvent) {
    if (!editor || status !== 'mounted') return
    const target = e.target as HTMLElement | null
    if (target !== e.currentTarget && target !== host) return
    e.preventDefault()
    focusRichEditorAtEnd(editor.view as any, 0)
  }

  // ── Search / Replace state ──
  interface MatchPos { from: number; to: number }
  let searchMatches: MatchPos[] = []
  let searchIndex = -1
  let lastSearchRegex = false
  let lastSearchPattern = ''
  let lastSearchCS = false

  let hoverYaml = $derived.by(() => {
    void hoverStore.version
    const t = activeTab()
    if (!t?.filePath) return null
    return getDisplayYaml(t.filePath)
  })

  // Auto-load yaml when this rich tab activates and mdblock is enabled.
  // SourceView has the same effect; without it here, opening a doc
  // directly into rich mode wouldn't trigger any load until the user
  // toggles to source or runs Cmd+Shift+B manually.
  $effect(() => {
    const t = activeTab()
    if (t?.filePath?.endsWith('.md') && isHoverActive()) {
      void loadHoverYaml(t.filePath)
    }
  })

  // Debounced live recompute when the rich editor's content changes.
  // Mirrors SourceView so users editing in rich also see structural
  // updates (new blocks, removed blocks, line shifts) within ~250 ms
  // of pausing typing.
  let richRecomputeTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    void tab.currentContent
    if (!tab.filePath || !isHoverActive() || !tab.filePath.endsWith('.md')) return
    if (richRecomputeTimer) clearTimeout(richRecomputeTimer)
    const filePath = tab.filePath
    const cur = tab.currentContent
    richRecomputeTimer = setTimeout(() => {
      void recomputeLiveYaml(filePath, cur)
    }, 250)
  })
  /**
   * Last value either pushed *out* of the editor (via onChange) or pulled
   * *into* it (via inbound resync). Lets us tell "editor has user edits not
   * yet propagated" from "editor and tab.currentContent already agree".
   * Without this:
   *   - the inbound $effect would loop on every onChange round-trip;
   *   - the destroy-flush would silently overwrite externally-replaced
   *     content with the editor's pre-replacement state.
   */
  let lastSync: string | null = null

  let _pmEl: HTMLElement | null = null
  let _dragDropUnlisten: (() => void) | null = null
  let _dragoverHandler: ((e: Event) => void) | null = null
  let _dropHandler: ((e: Event) => void) | null = null
  let _flushDocHandler: (() => void) | null = null

  let showImageToolbar = $state(false)
  let imageToolbarPosition = $state({ top: 0, left: 0 })
  let imageToolbarCurrentWidth = $state('')
  let imageToolbarTargetPos = $state<number | null>(null)

  // ── Slash menu state ─────────────────────────────────────────────────────────
  let showSlashMenu    = $state(false)
  let slashMenuPos     = $state({ top: 0, left: 0 })
  let slashItems       = $state<SlashItem[]>(getSlashItems())
  let slashSelectedIdx = $state(0)

  // ── Context menu state ───────────────────────────────────────────────────────
  let showCtxMenu   = $state(false)
  let ctxMenuPos    = $state({ x: 0, y: 0 })
  let ctxHasSel     = $state(false)
  let ctxImage      = $state(false)
  let ctxActions    = $state<EditorActions | null>(null)

  async function handlePaste(event: ClipboardEvent) {
    if (readOnly || !editor || !event.clipboardData) return

    // ── 1. Binary blob in clipboard (screenshot, copied image from browser) ──
    const items = Array.from(event.clipboardData.items)
    const binaryItem = items.find(item => item.kind === 'file')
    if (binaryItem) {
      const file = binaryItem.getAsFile()
      if (file) {
        event.preventDefault()
        event.stopImmediatePropagation()
        try {
          const path = await saveClipboardResource(file, tab.filePath)
          const view = editor.view as unknown as EditorView
          if (binaryItem.type.startsWith('image/')) {
            insertImageAtCursor(view, path)
          } else {
            insertAttachmentLink(view, path)
          }
        } catch (e) {
          console.warn('[RichEditor] paste save failed:', e)
        }
        return
      }
    }

    // ── 2. URL paste (video or attachment) ──
    const text = event.clipboardData.getData('text/plain')?.trim()
    if (text && isVideoUrl(text) && /^https?:\/\//.test(text)) {
      event.preventDefault()
      event.stopImmediatePropagation()
      const view = editor.view as unknown as EditorView
      // Insert placeholder link immediately
      insertAttachmentLink(view, text)
      // Async: fetch real title and replace the placeholder link text
      fetchVideoInfo(text).then(info => {
        if (!info || !editor) return
        const v = editor.view as unknown as EditorView
        const { doc } = v.state
        let replaceTr = v.state.tr
        let updated = false
        doc.descendants((node, pos) => {
          if (updated) return false
          const linkMark = node.marks.find(m => m.type.name === 'link' && m.attrs.href === text)
          if (linkMark && node.isText && node.text === text) {
            const newText = v.state.schema.text(info.title, node.marks)
            replaceTr = replaceTr.replaceWith(pos, pos + node.nodeSize, newText)
            updated = true
            return false
          }
        })
        if (updated) v.dispatch(replaceTr)
      }).catch(() => {})
      return
    }
    if (text && isAttachmentUrl(text)) {
      try { new URL(text) } catch { return }
      event.preventDefault()
      event.stopImmediatePropagation()
      const view = editor.view as unknown as EditorView
      insertAttachmentLink(view, text)
    }
    // 3. Everything else: let ProseMirror handle
  }

  async function setupDragDrop() {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    return getCurrentWebview().onDragDropEvent(async (event) => {
      if (readOnly || event.payload.type !== 'drop' || !editor) return
      const { paths, position } = event.payload

      const view = editor.view as unknown as EditorView
      let dropPos: number | null = null
      try {
        const result = view.posAtCoords({ left: position.x, top: position.y })
        if (result) dropPos = result.pos
      } catch { /* fallback: insert at cursor */ }

      for (const path of paths) {
        if (isImageExt(path)) {
          dropPos !== null
            ? insertImageAtPos(view, path, dropPos)
            : insertImageAtCursor(view, path)
        } else if (isAttachmentExt(path)) {
          insertAttachmentLink(view, path, dropPos ?? undefined)
        }
      }
    })
  }

  /** Click on a note badge (annotation widget or note_anchor node) → edit bubble. */
  function handleNoteClick(e: MouseEvent) {
    const target = e.target as HTMLElement
    const badge = target.closest('.note-badge, .moraya-note-anchor') as HTMLElement | null
    if (!badge || !editor) return
    e.preventDefault()
    e.stopPropagation()
    const view = editor.view as unknown as EditorView
    const rect = badge.getBoundingClientRect()
    const pos = view.posAtDOM(badge, 0)
    if (badge.classList.contains('moraya-note-anchor')) {
      // posAtDOM may resolve just inside/after the atom — probe both sides.
      const node = view.state.doc.nodeAt(pos)
      if (node?.type.name === 'note_anchor') openEditForAnchor(view, pos, rect)
      else openEditForAnchor(view, pos - 1, rect)
    } else {
      // Badge widget sits AFTER the annotated range → look left of it.
      openEditForMark(view, pos - 1, rect)
    }
  }

  /** Hover over anything carrying data-note → floating preview. */
  function handleNoteHover(e: MouseEvent) {
    const el = (e.target as HTMLElement).closest('[data-note]') as HTMLElement | null
    if (!el || !el.dataset.note) { noteUi.hover = null; return }
    const rect = el.getBoundingClientRect()
    noteUi.hover = { x: rect.left, y: rect.bottom + 4, note: el.dataset.note, style: readThemeStyle(el) }
  }

  function handleImageClick(event: MouseEvent) {
    const target = event.target as HTMLElement
    if (target.tagName !== 'IMG') {
      showImageToolbar = false
      return
    }

    const imgEl = target as HTMLImageElement
    const rect = imgEl.getBoundingClientRect()
    imageToolbarPosition = {
      top:  rect.top - 36,
      left: rect.left + rect.width / 2,
    }

    const titleAttr = imgEl.getAttribute('title') || ''
    const widthMatch = titleAttr.match(/^width=(\d+%?)$/)
    imageToolbarCurrentWidth = widthMatch ? widthMatch[1] : ''

    if (editor) {
      try {
        const view = editor.view as unknown as import('prosemirror-view').EditorView
        const pos = view.posAtDOM(imgEl, 0)
        imageToolbarTargetPos = pos
      } catch {
        imageToolbarTargetPos = null
      }
    }

    showImageToolbar = true
  }

  /**
   * Own link interaction in rich mode. This runs on `mousedown` in the capture
   * phase, ahead of @moraya/core's own ProseMirror handler (which otherwise
   * expands the link to editable source on a plain click and opens it on
   * Cmd/Ctrl-click — the opposite of what we want here).
   *
   *  - plain left-click → follow the link (URLs → system browser, editable
   *    files → new tab, other local files → system default app)
   *  - Cmd/Ctrl + click → edit: place the caret at the click point instead of
   *    navigating, so the user can modify the link text/target.
   */
  function handleLinkMouseDown(event: MouseEvent) {
    if (event.button !== 0) return
    const target = event.target as HTMLElement

    // 脚注定义前的编号(CSS ::before,画在定义块左侧 padding 里)→ 打开这条来源
    // 指向的地址。脚注在 vault 里基本都是出处,`[^loop]: /2026-07-27-….md（说明）`
    // 意思是"该论断出自那篇笔记",所以点它期待的是打开来源本身。
    // 命中判据是 target 恰为定义块:padding 区域(编号所在)属于块自身,而定义正文
    // 在内部的 <p> 里 —— 这样点文字仍能正常选中编辑。
    // 脚注的点击交给 @moraya/core 的 footnote-plugin 处理(正文角标 → 定义块,
    // 定义块编号 → 回跳首次引用)。这里不拦截。

    const wiki = target.closest('[data-wikilink]') as HTMLElement | null
    const urlEl = wiki ? null : (target.closest('[data-url]') as HTMLElement | null)
    const anchor = wiki || urlEl ? null : (target.closest('a[href]') as HTMLAnchorElement | null)
    if (!wiki && !urlEl && !anchor) return
    // Take full control of this event so moraya's mousedown handler never runs.
    event.preventDefault()
    event.stopImmediatePropagation()

    if (event.metaKey || event.ctrlKey) {
      // Frontmatter link labels hide their Markdown/wikilink source until the
      // editable value is focused. This NodeView has no ProseMirror contentDOM,
      // so placeCaretAtPoint cannot enter it; focus the value directly and put
      // the caret after the now-visible raw source instead.
      const fmValue = target.closest('.frontmatter-properties .fm-val.fm-editable') as HTMLElement | null
      if (fmValue) {
        fmValue.focus()
        const selection = window.getSelection()
        if (selection) {
          const range = document.createRange()
          range.selectNodeContents(fmValue)
          range.collapse(false)
          selection.removeAllRanges()
          selection.addRange(range)
        }
        return
      }
      placeCaretAtPoint(event.clientX, event.clientY)
      return
    }
    if (wiki) {
      void openWikilink(wiki.getAttribute('data-wikilink') || '')
      return
    }
    const href = urlEl ? urlEl.getAttribute('data-url') || '' : anchor!.getAttribute('href') || ''
    const action = classifyLink(href, tab.filePath)
    if (action.kind !== 'ignore') void openLinkAction(action)
  }

  /** Open a `[[wikilink]]` target, creating an empty `.md` file if it's missing. */
  async function openWikilink(name: string) {
    const abs = resolveWikilinkPath(name, tab.filePath)
    if (!abs) {
      const { showError } = await import('../lib/dialogs')
      showError('Save this document first to follow [[wikilinks]].')
      return
    }
    try {
      const { exists, writeTextFile } = await import('@tauri-apps/plugin-fs')
      if (!(await exists(abs))) {
        // 0 字节文件违反 OKF §4.1(必须有可解析 frontmatter + 非空 type),
        // 而且在索引里落进最低档 Unlabeled。走和大纲面板同一条建页路径。
        const { newWikilinkFileText } = await import('../lib/link-open')
        await writeTextFile(abs, await newWikilinkFileText(abs))
      }
      await openFile(abs)
    } catch (e) {
      const { showError } = await import('../lib/dialogs')
      showError(String(e))
    }
  }

  /** Move the caret to the document position under the given viewport coords. */
  function placeCaretAtPoint(clientX: number, clientY: number) {
    const view = editor?.view as EditorView | undefined
    if (!view) return
    const coords = view.posAtCoords({ left: clientX, top: clientY })
    if (!coords) return
    // prosemirror-state is already loaded (moraya mounts it), so this resolves
    // synchronously from cache — no perceptible delay.
    void import('prosemirror-state').then(({ TextSelection }) => {
      const sel = TextSelection.near(view.state.doc.resolve(coords.pos))
      view.dispatch(view.state.tr.setSelection(sel))
      view.focus()
    })
  }

  async function openLinkAction(action: LinkAction) {
    try {
      if (action.kind === 'browser') {
        const { openUrl } = await import('@tauri-apps/plugin-opener')
        await openUrl(action.url)
      } else if (action.kind === 'system') {
        const { openPath } = await import('@tauri-apps/plugin-opener')
        await openPath(action.path)
      } else if (action.kind === 'edit') {
        await openFile(action.path)
      }
    } catch (e) {
      const { showError } = await import('../lib/dialogs')
      showError(String(e))
    }
  }

  function handleRichContextMenu(event: MouseEvent) {
    if (!editor) return
    event.preventDefault()
    noteUi.hover = null
    const view = editor.view as unknown as EditorView
    const imgEl = (event.target as HTMLElement).closest('img') as HTMLImageElement | null
    ctxHasSel   = !view.state.selection.empty
    ctxImage    = !!imgEl
    ctxActions  = createRichActions(view, imgEl ? { imageEl: imgEl } : {})
    ctxMenuPos  = { x: event.clientX, y: event.clientY }
    showCtxMenu = true
  }

  function handleToolbarResize(width: string) {
    if (!editor || imageToolbarTargetPos === null) return
    try {
      const view = editor.view as unknown as import('prosemirror-view').EditorView
      const pos = imageToolbarTargetPos!
      const node = view.state.doc.nodeAt(pos)
      if (!node || node.type.name !== 'image') return
      const title = width ? `width=${width}` : ''
      view.dispatch(view.state.tr.setNodeMarkup(pos, undefined, { ...node.attrs, title }))
    } catch { /* ignore */ }
    imageToolbarCurrentWidth = width
  }

  function closeSlashMenu() {
    showSlashMenu    = false
    slashItems       = getSlashItems()
    slashSelectedIdx = 0
  }

  function checkSlashMenu() {
    if (!editor) return
    const view = editor.view as unknown as EditorView
    const fromPos = view.state.selection.$from

    if (fromPos.parent.type.name !== 'paragraph') { closeSlashMenu(); return }

    const textToCursor = fromPos.parent.textBetween(0, fromPos.parentOffset, '')
    const match = /^\/([a-zA-Z0-9一-龥]*)$/.exec(textToCursor)
    if (!match) { closeSlashMenu(); return }

    const coords = view.coordsAtPos(fromPos.pos)
    slashItems       = filterSlashItems(match[1])
    slashMenuPos     = { top: coords.bottom, left: coords.left }
    slashSelectedIdx = 0
    showSlashMenu    = true
  }

  function executeSlashItem(item: SlashItem) {
    if (!editor) return
    const view = editor.view as unknown as EditorView
    const fromPos = view.state.selection.$from
    // Delete '/' + filter text (from paragraph start to cursor)
    view.dispatch(view.state.tr.delete(fromPos.start(), fromPos.pos))
    item.execute(view)
    closeSlashMenu()
  }

  /** 变换区间(含结束那一下按键)的守卫,见 src/lib/ime.ts */
  const ime = createImeGuard()

  function handleRichKeydown(event: KeyboardEvent) {
    // ── IME first ──
    // This listener is registered in the CAPTURE phase, so it sees the key
    // before ProseMirror does. While an IME is composing, ↑↓/Enter/Esc drive
    // the candidate window — letting the slash menu eat them (or running a
    // shortcut) handles the same keystroke twice. `ime.blocks` also covers the
    // key that *closes* the composition, which some webviews deliver after
    // `compositionend` with no IME marking left on it. See src/lib/ime.ts.
    if (ime.blocks(event)) return

    // ── Slash menu navigation (highest priority) ──
    if (showSlashMenu) {
      if (event.key === 'ArrowDown') {
        event.preventDefault(); event.stopImmediatePropagation()
        slashSelectedIdx = Math.min(slashSelectedIdx + 1, slashItems.length - 1)
        return
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault(); event.stopImmediatePropagation()
        slashSelectedIdx = Math.max(slashSelectedIdx - 1, 0)
        return
      }
      if ((event.key === 'Enter' || event.key === 'Tab') && slashItems.length > 0) {
        event.preventDefault(); event.stopImmediatePropagation()
        executeSlashItem(slashItems[slashSelectedIdx])
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault(); event.stopImmediatePropagation()
        closeSlashMenu()
        return
      }
    }

    if (!editor) return
    const mod   = event.metaKey || event.ctrlKey
    const shift = event.shiftKey
    const alt   = event.altKey
    const key   = event.key.toLowerCase()
    const view  = editor.view as unknown as EditorView

    const s = view.state
    const sc = s.schema.nodes

    // ── Select All: Cmd+A ──
    // Handled here rather than left to the editor's own `Mod-a`: moraya-core
    // binds that to a raw AllSelection (which WebKit paints clamped to the
    // frontmatter block) and narrows it to the enclosing code_block when the
    // caret is inside one. Both look like "Cmd+A only selected part of the
    // document". Routing it through the shared helper gives the keyboard the
    // exact selection the right-click menu applies.
    if (handleSelectAllKeydown(event, view)) return

    // ── Heading shortcuts: Cmd+1-6 ──
    if (mod && !shift && !alt && /^[1-6]$/.test(event.key)) {
      event.preventDefault()
      if (sc.heading) setBlockType(sc.heading, { level: parseInt(event.key) })(s, view.dispatch)
      view.focus(); return
    }

    // ── Paragraph: Cmd+0 ──
    if (mod && !shift && !alt && event.key === '0') {
      event.preventDefault()
      if (sc.paragraph) setBlockType(sc.paragraph)(s, view.dispatch)
      view.focus(); return
    }

    // ── Code block: Cmd+Shift+K ──
    if (mod && shift && !alt && key === 'k') {
      event.preventDefault()
      if (sc.code_block) setBlockType(sc.code_block, { language: '' })(s, view.dispatch)
      view.focus(); return
    }

    // ── Insert annotation: Cmd+Shift+N ──
    // (Cmd+Shift+M is taken by math block below.)
    if (mod && shift && !alt && key === 'n') {
      event.preventDefault()
      insertNoteRich(view)
      return
    }

    // ── Ask (annotation seeded with `?`): Cmd+? ──
    // Matched on `key`, not `code`: `?` is Shift+/ on US layouts but sits
    // elsewhere on others — the printed character is what the user aims for.
    if (mod && !alt && event.key === '?') {
      event.preventDefault()
      insertNoteRich(view, '?')
      return
    }

    // ── Math block: Cmd+Shift+M ──
    if (mod && shift && !alt && key === 'm') {
      event.preventDefault()
      if (sc.math_block) view.dispatch(s.tr.replaceSelectionWith(sc.math_block.create({ value: '' })).scrollIntoView())
      view.focus(); return
    }

    // ── Table: Cmd+Shift+T ──
    if (mod && shift && !alt && key === 't') {
      event.preventDefault()
      const { table, table_header_row, table_row, table_header, table_cell, paragraph } = sc
      if (table && table_header_row && table_row && table_header && table_cell && paragraph) {
        const rows = 3, cols = 3
        const ep  = () => paragraph.createAndFill()!
        const hc  = () => table_header.createAndFill({ alignment: 'left' }, [ep()])!
        const bc  = () => table_cell.createAndFill(  { alignment: 'left' }, [ep()])!
        const tbl = table.create(null, [
          table_header_row.create(null, Array.from({ length: cols }, hc)),
          ...Array.from({ length: rows - 1 }, () => table_row.create(null, Array.from({ length: cols }, bc))),
        ])
        view.dispatch(s.tr.replaceSelectionWith(tbl).scrollIntoView())
      }
      view.focus(); return
    }

    // ── Blockquote: Cmd+Shift+Q ──
    if (mod && shift && !alt && key === 'q') {
      event.preventDefault()
      if (sc.blockquote) wrapIn(sc.blockquote)(s, view.dispatch)
      view.focus(); return
    }

    // ── Bullet list: Cmd+Opt+U ──
    if (mod && !shift && alt && key === 'u') {
      event.preventDefault()
      if (sc.bullet_list) wrapInList(sc.bullet_list)(s, view.dispatch)
      view.focus(); return
    }

    // ── Ordered list: Cmd+Opt+O ──
    if (mod && !shift && alt && key === 'o') {
      event.preventDefault()
      if (sc.ordered_list) wrapInList(sc.ordered_list)(s, view.dispatch)
      view.focus(); return
    }

    // ── Task list: Cmd+Opt+X ──
    if (mod && !shift && alt && key === 'x') {
      event.preventDefault()
      if (sc.bullet_list && sc.list_item) {
        wrapInList(sc.bullet_list)(s, view.dispatch)
        const s2 = view.state
        const tr = s2.tr
        s2.doc.nodesBetween(s2.selection.from - 200, s2.selection.to + 200, (node, pos) => {
          if (node.type === sc.list_item && node.attrs.checked === null)
            tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: false })
        })
        if (tr.docChanged) view.dispatch(tr)
      }
      view.focus(); return
    }
  }

  function unwrapIfNeeded(md: string): string {
    // Code-kind tabs pass through the fence stripper untouched. Markdown tabs
    // get wikilink brackets un-escaped so `[[name]]` persists literally.
    return wrapAsCodeBlock !== undefined ? stripCodeFence(md) : restoreWikilinks(md)
  }

  function wrapIfNeeded(md: string): string {
    if (readOnly && tab.kind === 'mdx') return toDisplayMarkdown(md)
    return wrapAsCodeBlock !== undefined ? buildFencedBlock(md, wrapAsCodeBlock) : md
  }

  // ── Search engine (ProseMirror decorations) ──

  function buildFlatText(doc: any): { text: string; offsets: number[] } {
    const parts: string[] = []
    const offsets: number[] = []
    let first = true
    doc.descendants((node: any, pos: number) => {
      if (node.isBlock && node.isTextblock) {
        if (!first) {
          parts.push('\n')
          offsets.push(-1)
        }
        first = false
        node.forEach((child: any, childOffset: number) => {
          if (child.isText && child.text) {
            for (let i = 0; i < child.text.length; i++) {
              parts.push(child.text[i])
              offsets.push(pos + 1 + childOffset + i)
            }
          }
        })
        return false
      }
      return true
    })
    return { text: parts.join(''), offsets }
  }

  function flatRangeToPmRanges(offsets: number[], start: number, end: number): MatchPos[] {
    const ranges: MatchPos[] = []
    let segStart = -1
    for (let i = start; i < end; i++) {
      if (offsets[i] === -1) {
        if (segStart >= 0) {
          ranges.push({ from: segStart, to: offsets[i - 1] + 1 })
          segStart = -1
        }
      } else {
        if (segStart < 0) segStart = offsets[i]
      }
    }
    if (segStart >= 0 && end > start) {
      for (let i = end - 1; i >= start; i--) {
        if (offsets[i] !== -1) {
          ranges.push({ from: segStart, to: offsets[i] + 1 })
          break
        }
      }
    }
    return ranges
  }

  function escapeRegex(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  }

  function findTextMatches(query: string, cs: boolean, wholeWord: boolean, useRegex: boolean): MatchPos[] {
    if (!editor || !query) return []
    const view = editor.view as any
    const { text: flatText, offsets } = buildFlatText(view.state.doc)

    let pattern = useRegex ? query : escapeRegex(query)
    if (wholeWord) pattern = `\\b${pattern}\\b`
    let regex: RegExp
    try {
      regex = new RegExp(pattern, cs ? 'g' : 'gi')
    } catch { return [] }

    const matches: MatchPos[] = []
    let m: RegExpExecArray | null
    while ((m = regex.exec(flatText)) !== null) {
      if (m[0].length === 0) { regex.lastIndex++; continue }
      const pmRanges = flatRangeToPmRanges(offsets, m.index, m.index + m[0].length)
      if (pmRanges.length > 0) {
        matches.push({ from: pmRanges[0].from, to: pmRanges[pmRanges.length - 1].to })
      }
      if (matches.length >= 10000) break
    }
    return matches
  }

  let _pmView: typeof import('prosemirror-view') | null = null
  let _pmState: typeof import('prosemirror-state') | null = null

  async function getPmView() {
    if (!_pmView) _pmView = await import('prosemirror-view')
    return _pmView
  }
  async function getPmState() {
    if (!_pmState) _pmState = await import('prosemirror-state')
    return _pmState
  }

  async function applySearchDecorations(matches: MatchPos[], activeIdx: number) {
    if (!editor) return
    const view = editor.view as any
    const { Decoration, DecorationSet } = await getPmView()
    if (matches.length === 0) {
      view.setProps({ decorations: () => DecorationSet.empty })
      return
    }
    const decos = matches.map((m, i) =>
      Decoration.inline(m.from, m.to, {
        class: i === activeIdx ? 'search-highlight-current' : 'search-highlight',
      })
    )
    const decoSet = DecorationSet.create(view.state.doc, decos)
    view.setProps({ decorations: () => decoSet })
  }

  async function scrollToMatch(idx: number) {
    if (!editor || idx < 0 || idx >= searchMatches.length) return
    const view = editor.view as any
    const { TextSelection } = await getPmState()
    const match = searchMatches[idx]
    const tr = view.state.tr.setSelection(TextSelection.create(view.state.doc, match.from, match.to))
    view.dispatch(tr)

    // Scroll the pane ourselves instead of relying on tr.scrollIntoView().
    // ProseMirror walks up from the *DOM selection's* node to find something
    // scrollable; while the find bar holds focus that node is the find input,
    // so PM climbs the find bar's ancestors and the editor never moves. The
    // scroll container is `.scroll` — `host` inside it never scrolls.
    requestAnimationFrame(() => {
      try {
        const scroller = (host as HTMLElement | null)?.closest('.scroll') as HTMLElement | null
        if (!scroller) return
        const coords = view.coordsAtPos(match.from)
        const rect = scroller.getBoundingClientRect()
        const margin = rect.height / 4
        if (coords.top < rect.top + margin || coords.bottom > rect.bottom - margin) {
          scroller.scrollTop += coords.top - rect.top - rect.height / 3
        }
      } catch { /* coordsAtPos can throw on a detached//re-rendering view */ }
    })
  }

  function getMatchedFlatText(doc: any, match: MatchPos): string {
    const parts: string[] = []
    doc.nodesBetween(match.from, match.to, (node: any, pos: number) => {
      if (node.isTextblock) {
        if (parts.length > 0) parts.push('\n')
        const startInNode = Math.max(match.from - pos - 1, 0)
        const endInNode = Math.min(match.to - pos - 1, node.content.size)
        if (endInNode > startInNode) {
          parts.push(node.textBetween(startInNode, endInNode))
        }
        return false
      }
      return true
    })
    return parts.join('')
  }

  import { findState } from '../lib/find-replace.svelte'
  import { reveal } from '../lib/outline/reveal.svelte'
  import { consumeEditorFocus } from '../lib/editor-focus.svelte'

  // Starts at 0, NOT at whatever is already pending: this component is rebuilt
  // by `{#key tab.id}` on every file switch, so a request issued just before
  // the switch (the search panel does exactly that — `openFile` then reveal)
  // is always older than this instance. Seeding from `reveal.req.seq` swallowed
  // it. `req.path` is what keeps that safe: a request aimed at another file is
  // declined rather than applied to whatever happens to be open.
  let lastRevealSeq = 0
  $effect(() => {
    const req = reveal.req
    // `status` is part of the condition on purpose — the view mounts through
    // an async import, so `host` exists well before the document is in the
    // DOM and a TreeWalker run before then finds nothing. The effect re-runs
    // when `status` flips, which is the first moment there is text to find.
    if (!req || req.seq === lastRevealSeq || !host || status !== 'mounted') return
    if (req.path && req.path !== tab.filePath) return
    lastRevealSeq = req.seq
    // 渲染 DOM 中按锚文本查找第一个匹配的元素
    const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT)
    let target: Element | null = null
    while (walker.nextNode()) {
      const tn = walker.currentNode as Text
      if (tn.textContent && tn.textContent.includes(req.text)) { target = tn.parentElement; break }
    }
    if (target) {
      target.scrollIntoView({ behavior: 'smooth', block: 'center' })
      target.classList.add('outline-reveal-flash')
      setTimeout(() => target!.classList.remove('outline-reveal-flash'), 1200)
    }
  })

  function onFindSearch(e: Event) {
    const { query, caseSensitive, wholeWord, useRegex } = (e as CustomEvent).detail
    lastSearchPattern = query
    lastSearchCS = caseSensitive
    lastSearchRegex = useRegex
    if (!query) {
      searchMatches = []
      searchIndex = -1
      findState.matchCount = 0
      findState.currentMatch = 0
      void applySearchDecorations([], -1)
      return
    }
    searchMatches = findTextMatches(query, caseSensitive, wholeWord, useRegex)
    searchIndex = searchMatches.length > 0 ? 0 : -1
    findState.matchCount = searchMatches.length
    findState.currentMatch = searchMatches.length > 0 ? 1 : 0
    void applySearchDecorations(searchMatches, searchIndex)
    if (searchIndex >= 0) void scrollToMatch(searchIndex)
  }

  function onFindNext() {
    if (searchMatches.length === 0) return
    searchIndex = (searchIndex + 1) % searchMatches.length
    findState.currentMatch = searchIndex + 1
    void applySearchDecorations(searchMatches, searchIndex)
    void scrollToMatch(searchIndex)
  }

  function onFindPrev() {
    if (searchMatches.length === 0) return
    searchIndex = (searchIndex - 1 + searchMatches.length) % searchMatches.length
    findState.currentMatch = searchIndex + 1
    void applySearchDecorations(searchMatches, searchIndex)
    void scrollToMatch(searchIndex)
  }

  function onFindReplace(e: Event) {
    if (!editor || searchIndex < 0 || searchIndex >= searchMatches.length) return
    const { replacement } = (e as CustomEvent).detail
    const view = editor.view as any
    const match = searchMatches[searchIndex]

    let replaceText = replacement
    if (lastSearchRegex && lastSearchPattern) {
      try {
        const regex = new RegExp(lastSearchPattern, lastSearchCS ? '' : 'i')
        const matchedText = getMatchedFlatText(view.state.doc, match)
        replaceText = matchedText.replace(regex, replacement)
      } catch { /* literal fallback */ }
    }

    const tr = replaceText
      ? view.state.tr.replaceWith(match.from, match.to, view.state.schema.text(replaceText))
      : view.state.tr.delete(match.from, match.to)
    view.dispatch(tr)

    // Re-search after replace
    onFindSearch(new CustomEvent('', { detail: {
      query: lastSearchPattern, caseSensitive: lastSearchCS,
      wholeWord: findState.wholeWord, useRegex: lastSearchRegex,
    }}))
  }

  function onFindReplaceAll(e: Event) {
    if (!editor || searchMatches.length === 0) return
    const { replacement } = (e as CustomEvent).detail
    const view = editor.view as any
    let tr = view.state.tr

    if (lastSearchRegex && lastSearchPattern) {
      try {
        const regex = new RegExp(lastSearchPattern, lastSearchCS ? '' : 'i')
        for (let i = searchMatches.length - 1; i >= 0; i--) {
          const matchedText = getMatchedFlatText(view.state.doc, searchMatches[i])
          const replaceText = matchedText.replace(regex, replacement)
          tr = replaceText
            ? tr.replaceWith(searchMatches[i].from, searchMatches[i].to, view.state.schema.text(replaceText))
            : tr.delete(searchMatches[i].from, searchMatches[i].to)
        }
      } catch {
        for (let i = searchMatches.length - 1; i >= 0; i--) {
          tr = replacement
            ? tr.replaceWith(searchMatches[i].from, searchMatches[i].to, view.state.schema.text(replacement))
            : tr.delete(searchMatches[i].from, searchMatches[i].to)
        }
      }
    } else {
      for (let i = searchMatches.length - 1; i >= 0; i--) {
        tr = replacement
          ? tr.replaceWith(searchMatches[i].from, searchMatches[i].to, view.state.schema.text(replacement))
          : tr.delete(searchMatches[i].from, searchMatches[i].to)
      }
    }
    view.dispatch(tr)
    onFindClear()
  }

  function onFindClear() {
    searchMatches = []
    searchIndex = -1
    findState.matchCount = 0
    findState.currentMatch = 0
    void applySearchDecorations([], -1)
  }

  // Re-run an open find against a freshly mounted editor. Switching rich/source
  // (or tabs) mounts a new component, and the find bar only dispatches when the
  // query itself changes — so the query sitting in the box would otherwise
  // apply to nothing here, leaving a stale count and no highlights.
  // Tracks `status` only: reading findState here (and writing matchCount
  // through onFindSearch) would re-invalidate this effect from inside itself.
  $effect(() => {
    const ready = status === 'mounted'
    untrack(() => {
      if (!ready || !findState.open || !findState.query) return
      onFindSearch(new CustomEvent('', { detail: {
        query: findState.query,
        caseSensitive: findState.caseSensitive,
        wholeWord: findState.wholeWord,
        useRegex: findState.useRegex,
      } }))
    })
  })

  $effect(() => {
    window.addEventListener('notemd:find-search', onFindSearch)
    window.addEventListener('notemd:find-next', onFindNext)
    window.addEventListener('notemd:find-prev', onFindPrev)
    window.addEventListener('notemd:find-replace', onFindReplace)
    window.addEventListener('notemd:find-replace-all', onFindReplaceAll)
    window.addEventListener('notemd:find-clear', onFindClear)
    window.addEventListener('notemd:new-file-select', onNewFileSelect)
    window.addEventListener('notemd:select-all', onSelectAll)
    return () => {
      window.removeEventListener('notemd:find-search', onFindSearch)
      window.removeEventListener('notemd:find-next', onFindNext)
      window.removeEventListener('notemd:find-prev', onFindPrev)
      window.removeEventListener('notemd:find-replace', onFindReplace)
      window.removeEventListener('notemd:find-replace-all', onFindReplaceAll)
      window.removeEventListener('notemd:find-clear', onFindClear)
      window.removeEventListener('notemd:new-file-select', onNewFileSelect)
      window.removeEventListener('notemd:select-all', onSelectAll)
    }
  })

  // Clicking Edit ▸ Select All. Native `PredefinedMenuItem::select_all` no-ops
  // on this editor — WebKit's `selectAll:` responder action gets confused by
  // the ProseMirror DOM's mix of editable text and non-editable atom nodes
  // (images, math blocks, …) — so the item is a custom one broadcasting
  // `notemd:select-all` (see the Rust menu + App.svelte).
  //
  // The keyboard no longer comes through here: Cmd+A is handled in
  // handleRichKeydown, because a menu key-equivalent never reaches the webview
  // at all and the focus round-trip below is fragile. This path stays for the
  // mouse, where that round-trip is unavoidable.
  function onSelectAll(): void {
    if (!editor || status !== 'mounted') return
    const view = editor.view as any
    // Focus BEFORE writing the selection: clicking the menu item leaves the
    // webview as not-first-responder, and a selection written then lands in
    // state but never gets painted — nothing looks selected, yet Backspace
    // deletes the whole document.
    view.focus()
    applySelectAll(view)
    // Re-sync once the menu has finished dismissing: when focus returns to the
    // webview after our write, WebKit restores its own cached (collapsed) DOM
    // selection. view.focus() re-runs ProseMirror's selectionToDOM from state,
    // so this repaints without dispatching anything — no history churn.
    const resync = () => { if (editor && status === 'mounted') view.focus() }
    requestAnimationFrame(resync)
    setTimeout(resync, 60)
  }

  async function onNewFileSelect(_e: Event) {
    if (!editor || status !== 'mounted') return
    const view = editor.view as any
    const { TextSelection, AllSelection } = await getPmState()
    setTimeout(() => {
      try {
        const doc = view.state.doc
        // Select everything after the first block (heading)
        const firstBlock = doc.firstChild
        if (!firstBlock) return
        const from = firstBlock.nodeSize
        const to = doc.content.size
        if (from >= to) return
        const tr = view.state.tr.setSelection(TextSelection.create(doc, from, to))
        view.dispatch(tr)
        view.focus()
      } catch { /* ignore */ }
    }, 100)
  }

  onMount(() => {
    if (!host) {
      errorMsg = 'host element missing'
      status = 'error'
      return
    }
    const tabId = tab.id
    ;(async () => {
      try {
        const { mountRichEditor, updateDocumentBaseDir } = await import('../lib/editor-bridge')
        updateDocumentBaseDir(tab.filePath)
        const inst = await mountRichEditor(host!, wrapIfNeeded(tab.currentContent), (md) => {
          // Read-only tabs render a transformed copy of the file; letting that
          // copy flow back would overwrite the real content with its display
          // form. Belt to `editable: () => false`'s braces.
          if (readOnly) return
          const unwrapped = unwrapIfNeeded(md)
          lastSync = unwrapped
          setContent(tabId, unwrapped)
        }, ime)
        if (readOnly) {
          const v = inst.view as unknown as EditorView
          // `editable: false` only stops DOM-level input. Menu commands,
          // keymap shortcuts and paste all reach the doc through
          // `view.dispatch`, which it does not touch — so drop doc-changing
          // transactions at the single choke point instead of trying to
          // disable every entry point. Selection/scroll transactions still
          // apply, or the view would not respond to clicks at all.
          v.setProps({
            editable: () => false,
            dispatchTransaction: (tr) => {
              if (tr.docChanged) return
              v.updateState(v.state.apply(tr))
            },
          })
        }
        // Mark in-sync BEFORE exposing the editor: the inbound $effect runs
        // immediately on `status === 'mounted'`, and would otherwise see a
        // null lastSync and re-push the same content into a freshly-mounted
        // view (harmless but wasteful).
        lastSync = tab.currentContent
        editor = inst
        status = 'mounted'
        _pmEl = host!.querySelector('.ProseMirror') as HTMLElement | null

        // Create-then-focus flows (e.g. quick note): if this file was flagged
        // for focus, grab it now that the view exists — cursor at the doc end,
        // in edit state.
        if (consumeEditorFocus(tab.filePath)) {
          focusRichEditorAtEnd(inst.view as any)
        }

        // Append the wikilink decoration plugin. moraya's setContent only
        // dispatches transactions (never reconfigures), so this survives
        // inbound content syncs. prosemirror-* is already loaded by moraya,
        // so these dynamic imports resolve from cache.
        try {
          const view = inst.view as unknown as EditorView
          const { wikilinkPlugin } = await import('../lib/wikilink-plugin')
          const { noteBadgePlugin } = await import('../lib/note-anno/note-plugin')
          const { placeholderPlugin } = await import('../lib/placeholder-plugin')
          const { answerCardPlugin } = await import('../lib/note-anno/answer-card')
          const { answeredMap } = await import('../lib/note-anno/answers-store.svelte')
          const { adoptAnswer } = await import('../lib/note-anno/adopt-answer')
          const { powerModePlugin } = await import('../lib/power-mode/plugin')
          const { mainWindowConfig } = await import('../lib/power-mode/host-config.svelte')
          view.updateState(
            view.state.reconfigure({
              plugins: view.state.plugins.concat(
                wikilinkPlugin(),
                noteBadgePlugin(),
                // 已作答问题 → 被批注段落之后的可展开 ✦ 卡片(采纳才写源文件)
                answerCardPlugin({
                  getEntries: () => answeredMap(),
                  onAdopt: (entry, pos, v) => { void adoptAnswer(v, entry, pos, tab.filePath ?? null) },
                }),
                placeholderPlugin(t('editor.emptyPlaceholder')),
                // Power Mode:getter 每次击键现取,配置改了下一次输入就生效;
                // 生效面关着时返回 null,引擎整条链路直接短路。
                powerModePlugin(mainWindowConfig, () => tab.filePath ?? 'untitled'),
              ),
            }),
          )
        } catch (e) {
          console.warn('[RichEditor] wikilink plugin init failed:', e)
        }

        _pmEl?.addEventListener('paste', handlePaste, true)
        _pmEl?.addEventListener('click', handleNoteClick as EventListener, true)
        _pmEl?.addEventListener('mouseover', handleNoteHover as EventListener)
        _pmEl?.addEventListener('click', handleImageClick as EventListener)
        _pmEl?.addEventListener('mousedown', handleLinkMouseDown as EventListener, true)
        _pmEl?.addEventListener('keydown', handleRichKeydown as EventListener, true)
        _pmEl?.addEventListener('input',   checkSlashMenu as EventListener)
        _pmEl?.addEventListener('contextmenu', handleRichContextMenu as EventListener)

        // Prevent browser default file drop behaviour
        _dragoverHandler = (e) => e.preventDefault()
        _dropHandler     = (e) => e.preventDefault()
        host!.addEventListener('dragover', _dragoverHandler)
        host!.addEventListener('drop',     _dropHandler)

        // Annotation commands ask for an immediate doc→tab flush so the
        // outline panel updates without waiting for the lazy-change debounce.
        _flushDocHandler = () => {
          if (!editor) return
          const md = unwrapIfNeeded(editor.getMarkdown())
          lastSync = md
          setContent(tabId, md)
        }
        window.addEventListener('notemd:flush-doc', _flushDocHandler)

        // Tauri native file drag-drop
        setupDragDrop().then(fn => { _dragDropUnlisten = fn }).catch(console.warn)
      } catch (e) {
        console.error('[RichEditor] mount failed:', e)
        errorMsg = e instanceof Error ? `${e.name}: ${e.message}` : String(e)
        status = 'error'
      }
    })()
  })

  // Inbound sync: when tab.currentContent is replaced from outside the
  // editor (reloadFromDisk, future autoReload paths, etc.), push it into
  // the ProseMirror view. Round-trips from our own onChange are filtered
  // by `lastSync`.
  $effect(() => {
    const target = tab.currentContent
    if (status !== 'mounted' || !editor) return
    if (target === lastSync) return
    editor.setContent(wrapIfNeeded(target))
    lastSync = target
  })

  // Keep documentBaseDir in sync with the active file path so relative
  // image paths (e.g. report_files/image.png) resolve correctly.
  $effect(() => {
    const fp = tab.filePath
    if (status !== 'mounted') return
    import('../lib/editor-bridge').then(({ updateDocumentBaseDir }) => {
      updateDocumentBaseDir(fp)
    })
  })

  // 答复卡片(一):切换文档时按需加载该文档配套 .note.md 的答复索引。
  // 只加载当前文档,不预扫 vault;写 answersStore 的动作包在 untrack 里,
  // 避免「effect 内读+写 $state」自失效(见 v4.2.4 冻结 UI 教训)。
  $effect(() => {
    const fp = tab.filePath
    if (status !== 'mounted') return
    untrack(() => { void loadAnswersFor(fp) })
  })

  // 答复卡片(二):索引变化(加载完成 / 采纳后失效重建)→ 让插件重建 decoration。
  $effect(() => {
    void answersStore.version
    if (status !== 'mounted' || !editor) return
    untrack(() => {
      const view = editor!.view as unknown as EditorView
      void import('../lib/note-anno/answer-card').then(({ ANSWER_CARDS_REFRESH }) => {
        view.dispatch(view.state.tr.setMeta(ANSWER_CARDS_REFRESH, true))
      })
    })
  })

  $effect(() => {
    function onFocusEditor(ev: Event) {
      const d = (ev as CustomEvent<{ path: string }>).detail
      if (status !== 'mounted' || !editor || tab.filePath !== d.path) return
      focusRichEditorAtEnd(editor.view as any)
    }
    window.addEventListener('notemd:focus-editor', onFocusEditor)
    return () => window.removeEventListener('notemd:focus-editor', onFocusEditor)
  })

  onDestroy(() => {
    _pmEl?.removeEventListener('paste', handlePaste, true)
    _pmEl?.removeEventListener('click', handleNoteClick as EventListener, true)
    _pmEl?.removeEventListener('mouseover', handleNoteHover as EventListener)
    _pmEl?.removeEventListener('click', handleImageClick as EventListener)
    _pmEl?.removeEventListener('mousedown', handleLinkMouseDown as EventListener, true)
    _pmEl?.removeEventListener('keydown', handleRichKeydown as EventListener, true)
    _pmEl?.removeEventListener('input',   checkSlashMenu as EventListener)
    _pmEl?.removeEventListener('contextmenu', handleRichContextMenu as EventListener)
    _dragDropUnlisten?.()
    if (_flushDocHandler) window.removeEventListener('notemd:flush-doc', _flushDocHandler)
    host?.removeEventListener('dragover', _dragoverHandler!)
    host?.removeEventListener('drop',     _dropHandler!)
    if (editor) {
      try {
        const md = editor.getMarkdown()
        const unwrapped = unwrapIfNeeded(md)
        // Skip flush when the editor is already in sync with tab.currentContent
        // — flushing then would overwrite a just-arrived external replacement
        // with the editor's pre-replacement state. Only push when there are
        // genuinely unflushed user edits (debounce hasn't fired yet).
        if (unwrapped !== lastSync) onFlush?.(unwrapped)
        editor.destroy()
      } catch (e) {
        console.warn('[RichEditor] destroy failed:', e)
      }
      editor = null
    }
  })
</script>

<div class="rich-wrap">
  {#if status === 'error'}
    <div class="diag err">[error] {errorMsg ?? 'unknown'}</div>
  {/if}
  <div class="rich-pane">
    {#if isHoverActive() && settings.mdblock.hover.showRichOverlay && hoverYaml && host}
      <RichGutter container={host}
                  yaml={hoverYaml}
                  source={tab.currentContent}
                  pageBasename={(activeTab()?.filePath ?? '').replace(/^.*[\\/]/, '')} />
    {/if}
    <div class="scroll" onmousedown={onScrollMouseDown} role="presentation">
      <div class="host" data-theme={activeThemeId} bind:this={host}></div>
      {#if backlinkPage}
        <LinkedReferences page={backlinkPage} excludeFile={tab.filePath ?? null} />
      {/if}
    </div>
  </div>
  {#if showImageToolbar}
    <ImageToolbar
      position={imageToolbarPosition}
      currentWidth={imageToolbarCurrentWidth}
      onResize={handleToolbarResize}
      onClose={() => { showImageToolbar = false }}
    />
  {/if}
  {#if showSlashMenu}
    <SlashMenu
      position={slashMenuPos}
      items={slashItems}
      selectedIndex={slashSelectedIdx}
      onSelect={executeSlashItem}
      onClose={closeSlashMenu}
    />
  {/if}
  {#if showCtxMenu && ctxActions}
    <EditorContextMenu
      position={ctxMenuPos}
      hasSelection={ctxHasSel}
      image={ctxImage}
      {readOnly}
      actions={ctxActions}
      onClose={() => { showCtxMenu = false }}
    />
  {/if}
  <NotePopover />
  {#if noteUi.edit}
    <NoteEditPopup />
  {/if}
</div>

<style>
  .rich-wrap {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
  }
  .diag {
    flex-shrink: 0;
    padding: 4px 12px;
    font-family: ui-monospace, Menlo, monospace;
    font-size: 11px;
    background: color-mix(in srgb, CanvasText 8%, Canvas);
    color: GrayText;
  }
  .err { color: #c0392b; }
  .rich-pane {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  /* Scroll container: holds the editor host AND the inline backlinks so they
     scroll together as one page (horizontal inset lives here so both align). */
  .scroll {
    flex: 1;
    overflow: auto;
    min-height: 0;
    padding: 0 24px;
    box-sizing: border-box;
    /* No GPU compositing hints here. `will-change: transform` + translateZ(0) +
       `contain: paint` promoted this to its own composited, paint-clipped layer,
       and WebKit then never drew the caret inside a block with no text — an
       empty paragraph or a just-typed `# ` heading showed no cursor even though
       caret-color was opaque and the line box had full height. */
  }
  .host {
    padding: 16px 0;
    box-sizing: border-box;
    min-height: 200px;
    /* Flex column so the editor can stretch to the host's full height. A
       percentage min-height on the editor resolves against an auto-height
       parent as 0, which left a short/empty doc only one line tall — clicks
       below that line missed the editor entirely and placed no caret. */
    display: flex;
    flex-direction: column;
  }
  .host :global(.ProseMirror),
  .host :global(.moraya-editor) {
    outline: none;
    flex: 1 0 auto;
  }
  /* Blank space below the editor still reads (and behaves) as writing surface. */
  .scroll,
  .host {
    cursor: text;
  }
  /* Hint inside an otherwise blank document so the caret's home is visible.
     `float` keeps it out of layout flow without needing a positioned parent.
     Scoped here, so the Editor Kit (src/editor-kit/kit.css) carries its own
     copy — keep the two in sync. */
  .host :global(.ProseMirror .is-empty)::before {
    content: attr(data-placeholder);
    float: left;
    height: 0;
    pointer-events: none;
    color: color-mix(in srgb, CanvasText 40%, Canvas 60%);
  }
</style>
