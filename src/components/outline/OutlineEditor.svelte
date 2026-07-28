<script lang="ts">
  import type { Tab } from '../../lib/tabs.svelte'
  import { tabs, setContent, openFile } from '../../lib/tabs.svelte'
  import { outlineShortcuts } from '../../lib/outline/gate.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import OutlineNode from './OutlineNode.svelte'
  import SlashMenu from './SlashMenu.svelte'
  import LinkAutocomplete from './LinkAutocomplete.svelte'
  import NodeContextMenu from './NodeContextMenu.svelte'
  import LinkedReferences from './LinkedReferences.svelte'
  import {
    outline, attachDoc, detach, serializeDoc, setChangeSink, regenerate,
    bump, markDirty, markSynced, markSaved, pinnedIds, setSelection, clearSelection, companionPathFor,
    isEffectivelyEmptyTree, noteTextHasContent,
  } from '../../lib/outline/store.svelte'
  import { sha256Hex } from '../../lib/hash'
  import { decideCompanionWrite } from '../../lib/outline/companion-write'
  import { deriveAutoItems } from '../../lib/outline/derive'
  import { syncAutoItems } from '../../lib/outline/sync'
  import { childrenOf, newId, calculateOrderBetween, setNodeContent, treeHasQuestion, type OutlineNode as NodeT } from '../../lib/outline/model'
  import OutlineBreadcrumb from './OutlineBreadcrumb.svelte'
  import {
    moveNodeAfter, moveNodeToChild, deleteNode, subtreeToMarkdown,
    deleteNodes, indentNodes, outdentNodes, moveNodesAfter, moveNodesToChild, nodesToMarkdown,
  } from '../../lib/outline/commands'
  import { selectionRoots, rangeBetween } from '../../lib/outline/select'
  import { resolveShortcuts, type OutlineCommandId } from '../../lib/outline/shortcuts'
  import { filterSlashItems, applySlashItem, pageLinkQueryAt, confirmPageLink, filterPages, type SlashItem } from '../../lib/outline/completion'
  import { pageCandidates, pageNameOf } from '../../lib/outline/backlinks'
  import { activeTheme } from '../../lib/active-theme.svelte'
  import { requestReveal } from '../../lib/outline/reveal.svelte'
  import { ensureIndex, openPageOrCreate } from '../../lib/outline/backlinks-io.svelte'
  import { whenWikilinkBlocklistReady } from '../../lib/wikilink/blocklist-io.svelte'
  import { untrack } from 'svelte'
  import { noteHomeForRead, planNoteHome } from '../../lib/outline/note-home'
  import { sotvaultStore, syncSourceToVaultAsHome, refreshSotvault } from '../../lib/sotvault.svelte'
  import { openSettings } from '../../lib/ui-state.svelte'
  import { decideCompanionReload } from '../../lib/outline/companion-reload'
  import { captureScroll } from '../../lib/scroll-keep'
  import { setAnswersFromText, answersStore } from '../../lib/note-anno/answers-store.svelte'

  let { tab = null, mainTab = null, embedded = false, onWikilink = null, onCollapse = null, linkedRefs = null, focusRootId = undefined, onFocusChange = null }: {
    /** tab 模式：单独打开的 .note.md tab —— 纯大纲编辑器 */
    tab?: Tab | null
    /** panel 模式：正在编辑的主文档 tab —— 全功能大纲，随主文档实时同步 */
    mainTab?: Tab | null
    /** embedded 模式（如每日笔记窗口）：隐藏标题+工具栏，只显示大纲区域，
     *  与只读渲染保持一致。默认 false，主窗口行为不变。 */
    embedded?: boolean
    /** 双链点击处理器覆盖（每日笔记窗口用它做窗口内导航，而非默认的
     *  openPageOrCreate 打开主窗口）。null = 默认行为。 */
    onWikilink?: ((target: string) => void) | null
    /** 折叠切换处理器覆盖（每日笔记窗口用它把折叠存进 KV 而非 .note.md，
     *  避免 markDirty 触发文件写）。null = 默认（bump+markDirty，写文件）。 */
    onCollapse?: ((n: NodeT) => void) | null
    /** 是否显示底部 Linked References 区。null=跟随 embedded(嵌入时默认不显示);
     *  每日笔记的单页视图显式传 true 以与主大纲视图一致。 */
    linkedRefs?: boolean | null
    /** zoom 聚焦根节点 id。**省略(undefined)=非受控**:编辑器自管聚焦(主大纲/单页
     *  就地 zoom,自带面包屑)。**传入(string|null)=受控**:按此值渲染聚焦子树,自身
     *  不改状态、不渲染面包屑(由宿主管,如每日日志聚焦视图),仅通过 onFocusChange 通知。 */
    focusRootId?: string | null | undefined
    /** 聚焦变更(bullet 点击 zoom-in / 面包屑 zoom-out)时回调;受控模式下宿主据此更新 focusRootId。 */
    onFocusChange?: ((id: string | null) => void) | null
  } = $props()

  // 聚焦状态:受控(prop 非 undefined)用外部值;非受控用内部 $state。
  const focusControlled = $derived(focusRootId !== undefined)
  let internalFocus = $state<string | null>(null)
  const effectiveFocus = $derived(focusControlled ? (focusRootId ?? null) : internalFocus)
  function focusNode(id: string | null) {
    if (!focusControlled) internalFocus = id
    onFocusChange?.(id)
  }

  // Embedded (Daily Notes feed) hides Linked References; a single-page view opts
  // back in via linkedRefs=true so it matches the main 大纲笔记 view.
  const showLinkedRefs = $derived(linkedRefs ?? !embedded)

  /** 伴生笔记的主文档路径 */
  let mainPath = $derived(tab ? tab.filePath.replace(/\.notes?\.md$/i, '.md') : mainTab!.filePath)
  /** 大纲 .note.md 路径（panel 模式来自主文档的伴生名） */
  let notePath = $derived(
    tab
      ? tab.filePath
      : (noteHomeForRead(mainTab!.filePath, { vaultRoot: sotvaultStore.vaultRoot, records: sotvaultStore.records })
          ?? companionPathFor(mainTab!.filePath)!)
  )
  /** 大纲文本落点：.note.md tab 开着 → 走 tab（保脏标记/撤销）；否则写盘 */
  let noteTab = $derived(tab ?? tabs.find(x => x.filePath === notePath) ?? null)
  let resolved = $derived(resolveShortcuts(outlineShortcuts.overrides))

  // panel 模式无 tab 时的落盘器（防抖 + 卸载冲刷）。空大纲且文件不存在时不落盘。
  let diskTimer: ReturnType<typeof setTimeout> | null = null
  let diskPending: string | null = null
  /** 冲突校验基线：我们上次加载/写入 .note.md 时的 sha256（null=当时磁盘无此文件） */
  let noteDiskHash: string | null = null
  /** In-flight first-write sync, keyed by source path — coalesces concurrent/rapid
   *  debounced flushes into a SINGLE sotvault sync so we never create a duplicate
   *  vault copy (the Rust side dedups filenames, so a 2nd sync would make `<stem>-2.md`). */
  let homeSync: { src: string; promise: Promise<string | null> } | null = null
  /** 写笔记前确定落点。null = 无 vault 且用户未配置 → 本次不写(内存保留)。
   *  panel 模式专用(tab 模式笔记本就是文件,无需 gate)。 */
  async function ensureNoteHome(): Promise<{ notePath: string; justSynced: boolean } | null> {
    const src = mainPath
    const fs = await import('@tauri-apps/plugin-fs')
    const companion = companionPathFor(src)
    const legacyNoteExists = companion ? await fs.exists(companion).catch(() => false) : false
    const plan = planNoteHome(src, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
      legacyNoteExists,
    })
    if (plan.action === 'use') return { notePath: plan.notePath, justSynced: false }
    if (plan.action === 'configure-vault') {
      const { ask } = await import('@tauri-apps/plugin-dialog')
      if (await ask(t('outline.vaultRequiredForNoteBody'), { title: t('outline.vaultRequiredForNote') })) {
        openSettings()
      }
      return null
    }
    // plan.action === 'sync' —— 首次写笔记:把源复制进 vault,笔记落 vault 副本旁。
    // 按 src 合并在途 sync:并发/refresh 前的重复首写共用同一次,绝不造第二份副本。
    if (!homeSync || homeSync.src !== src) {
      homeSync = {
        src,
        promise: syncSourceToVaultAsHome(src)
          .then((rec) => companionPathFor(rec.vault_path))
          .catch((e) => { console.warn('[outline] sync source to vault failed:', e); return null }),
      }
    }
    const home = await homeSync.promise
    return home ? { notePath: home, justSynced: true } : null
  }

  async function flushDisk() {
    if (diskTimer) { clearTimeout(diskTimer); diskTimer = null }
    const text = diskPending
    diskPending = null
    if (text == null) return
    if (outline.externalConflict) return               // 冲突未解决前不写
    try {
      const fs = await import('@tauri-apps/plugin-fs')
      // 空大纲不触发建家/同步(浏览/空树不得污染,也不得把源拷进 vault)
      if (text.trim() === '') {
        const existed0 = await fs.exists(notePath).catch(() => false)
        if (!existed0) return
      }
      const home = await ensureNoteHome()
      if (!home) return                                // 无 vault/用户取消 → 内存保留,不落盘
      const target = home.notePath
      const existed = await fs.exists(target).catch(() => false)
      if (text.trim() === '' && !existed) return
      // Data-loss guard: 不用空/空白序列化覆盖一个本来有内容的落点
      if (!noteTextHasContent(text) && existed) {
        const existing = await fs.readTextFile(target).catch(() => '')
        if (noteTextHasContent(existing)) return
      }
      const diskText = existed ? await fs.readTextFile(target).catch(() => null) : null
      const diskHash = diskText != null ? await sha256Hex(diskText) : null
      const ourHash = await sha256Hex(text)
      const decision = decideCompanionWrite({
        fileExists: diskText != null, diskHash, lastHash: noteDiskHash, ourHash,
      })
      if (decision === 'conflict') { outline.externalConflict = { diskText: diskText ?? '' }; return }
      if (decision === 'noop') { noteDiskHash = diskHash; markSaved(); return }
      await fs.writeTextFile(target, text)
      noteDiskHash = ourHash
      markSaved()
      // 刚建家:笔记已写进 vault 副本旁,刷新 records → 响应式 notePath 翻转到 vault 路径。
      // noteDiskHash 保持 ourHash——它正是 vault target 的磁盘内容哈希,翻转后依然是有效基线
      // (挂载 effect 重挂载时也会从该路径读回同一 hash),不清 null 以免 refresh 窗口内误报冲突。
      if (home.justSynced) await refreshSotvault()
    } catch (e) {
      console.warn('[outline] write companion failed:', e)
    }
  }
  function persistToDisk(text: string) {
    diskPending = text
    if (diskTimer) clearTimeout(diskTimer)
    diskTimer = setTimeout(() => { void flushDisk() }, 500)
  }

  // 挂载:解析大纲文本 → 派生同步 → 注册 sink。若派生/补 fm 改变了
  // 序列化结果,回写落点让脏标记如实反映。
  // cancelled 标志防止异步延迟到达的延续覆盖新 tab 的树/回写旧 id/装陈旧 sink。
  $effect(() => {
    const path = notePath
    const persistTab = noteTab
    let cancelled = false
    untrack(() => {
      void (async () => {
        // 大纲初始文本：tab / 镜像 tab / 磁盘
        let noteText = persistTab?.currentContent ?? null
        if (noteText == null) {
          const { exists, readTextFile } = await import('@tauri-apps/plugin-fs')
          if (cancelled) return
          if (await exists(path).catch(() => false)) {
            noteText = await readTextFile(path).catch(() => null)
          }
        }
        // 主文档内容：panel 模式直接取主 tab；tab 模式找 tab / 读盘
        let mainContent: string | null = mainTab?.currentContent ?? null
        if (mainContent == null) {
          const openMain = tabs.find(x => x.filePath === mainPath)
          if (openMain) mainContent = openMain.currentContent
          else {
            const { exists, readTextFile } = await import('@tauri-apps/plugin-fs')
            if (cancelled) return
            if (await exists(mainPath).catch(() => false)) {
              mainContent = await readTextFile(mainPath).catch(() => null)
            }
          }
        }
        if (cancelled) return
        // 首扫派生 wikilink 前先等黑名单加载完，否则空 Set 会漏过拉黑项并写进伴生笔记。
        await whenWikilinkBlocklistReady()
        if (cancelled) return
        await attachDoc(path, noteText ?? '', mainContent)
        if (cancelled) return   // 迟到的挂载不得覆盖新 tab 的树/回写旧 id/装陈旧 sink
        lastDerivedMain = mainContent
        const out = serializeDoc(false)
        // 数据丢失防线：全局树是单例，attach/detach + "空树自动补根节点" 竞态下 sink
        // 可能带着空树/空白根节点触发，把非空 note 覆盖成空。写回前一律校验：
        //   1) 全局树确实属于本笔记(docPath === path)；
        //   2) 不用"实质为空的树"覆盖一个本来有内容的落点。
        const wouldWipe = (existing: string): boolean =>
          outline.docPath !== path ||
          (!outline.allowEmptyWrite && isEffectivelyEmptyTree(outline.tree) && noteTextHasContent(existing))
        if (persistTab) {
          if (out !== persistTab.currentContent && !wouldWipe(persistTab.currentContent)) {
            setContent(persistTab.id, out)
          }
          setChangeSink(() => {
            if (wouldWipe(persistTab.currentContent)) return
            setContent(persistTab.id, serializeDoc())
          })
        } else {
          // panel-disk：种下冲突校验基线；不在挂载时自动写盘（避免“打开即写”/浏览即生成）。
          // “would this wipe an existing note?” 的实盘校验在 flushDisk 内（异步读 exists）。
          const fsMod = await import('@tauri-apps/plugin-fs')
          const existed0 = await fsMod.exists(path).catch(() => false)
          if (cancelled) return
          noteDiskHash = existed0
            ? await sha256Hex(await fsMod.readTextFile(path).catch(() => '')).catch(() => null)
            : null
          if (cancelled) return
          setChangeSink(() => { if (outline.docPath === path) persistToDisk(serializeDoc()) })
        }
      })()
    })
    return () => {
      cancelled = true
      untrack(() => { setChangeSink(null); void flushDisk(); detach() })
    }
  })

  // 主文档 → 大纲实时同步（panel 模式核心；tab 模式主 tab 开着时同样生效，
  // 覆盖批注回写等主文档变化）。防抖后仅内容真变才重派生。
  let lastDerivedMain: string | null = null
  let deriveTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    const mc = mainTab ? mainTab.currentContent
      : tabs.find(x => x.filePath === mainPath)?.currentContent
    if (mc == null) return
    if (deriveTimer) clearTimeout(deriveTimer)
    deriveTimer = setTimeout(() => {
      void (async () => {
        await whenWikilinkBlocklistReady()   // 黑名单未加载完前不派生，避免漏过滤
        untrack(() => {
          if (mc === lastDerivedMain) return
          lastDerivedMain = mc
          const before = serializeDoc(false)
          syncAutoItems(outline.tree, deriveAutoItems(mc))
          if (treeHasQuestion(outline.tree)) outline.armed = true
          // 同步只置脏、进内存；未激活自动保存时不落盘（浏览/主文档编辑不自动生成笔记）
          if (serializeDoc(false) !== before) { bump(); markSynced() }
        })
      })()
    }, 300)
    return () => { if (deriveTimer) clearTimeout(deriveTimer) }
  })

  // 外部变更自动重载(干净 tab)→ 重新解析
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as { tabId: string; newContent: string } | undefined
      // tab 模式匹配自己这个 tab;panel 模式下伴生笔记也可能同时被开成 tab,
      // 那时 file-watcher 重载的是它 —— 两种都要接,否则大纲树会停在旧内容。
      if (!detail || !noteTab || detail.tabId !== noteTab.id) return
      const path = notePath
      const mc = tab ? null : currentMainContent()
      untrack(() => {
        const restore = captureScroll(bodyEl)   // 重建整棵树会把滚动清零
        void attachDoc(path, detail.newContent, mc)
          .then(async () => {
            // 把基线推进到刚载入的内容,免得伴生监听把同一次变更再判成新变更
            noteDiskHash = await sha256Hex(detail.newContent).catch(() => null)
            if (answersStore.notePath === path) setAnswersFromText(path, detail.newContent)
            restore()
          })
          .catch((err) => console.warn('[outline] rebuild after reload failed:', err))
      })
    }
    window.addEventListener('mdeditor:auto-reloaded', handler)
    return () => window.removeEventListener('mdeditor:auto-reloaded', handler)
  })

  // 索引是共享单例:编辑器卸载不 teardown(面板可能还在用);
  // 追踪 backlinkIndex,被别处 teardown 置 null 时自愈重建
  $effect(() => { void outline.backlinkIndex; void ensureIndex(notePath) })

  // 跳转:伴生笔记的 auto 节点 → 打开主文档并 reveal 行号
  async function onJump(n: NodeT) {
    if (n.anchorLine == null) return
    await openFile(mainPath).catch(() => {})
    requestReveal(n.anchorLine, n.content)
  }
  function onPageClick(target: string) {
    if (onWikilink) onWikilink(target)
    else void openPageOrCreate(target)
  }

  // Theme-driven typography: measured from an offscreen probe (see effect below).
  let activeThemeId = $derived(activeTheme.id)
  let probeEl = $state<HTMLDivElement>()
  let typo = $state({ family: '', size: '', line: '', fg: '', bg: '' })

  // Re-measure triggers beyond an id change: the theme slot CSS loads
  // asynchronously (a mount-time measurement would otherwise stay stale on
  // system colors forever), and a light/dark flip re-resolves system colors
  // without any id/CSS change (stale light-mode text turned unreadable on the
  // dark Canvas).
  let themeCssTick = $state(0)
  $effect(() => {
    const mo = new MutationObserver((muts) => {
      for (const m of muts) {
        const el = m.target instanceof Element ? m.target : m.target.parentElement
        if (el?.closest('style[data-theme-slot]')) { themeCssTick++; break }
      }
    })
    mo.observe(document.head, { childList: true, characterData: true, subtree: true })
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onMq = () => { themeCssTick++ }
    mq.addEventListener('change', onMq)
    return () => { mo.disconnect(); mq.removeEventListener('change', onMq) }
  })

  // Read the theme's base body typography (font-family/size/line-height, which
  // live on `.moraya-editor` under `[data-theme=<id>]`) and expose as CSS vars.
  // rAF waits for the theme slot CSS to apply after an id change.
  $effect(() => {
    void activeThemeId
    void themeCssTick
    const probe = probeEl?.querySelector('.moraya-editor') as HTMLElement | null
    if (!probe) return
    const raf = requestAnimationFrame(() => {
      const cs = getComputedStyle(probe)
      // 同时取主题的前景/背景色，让面板跟随 theme（背景透明时保持系统 Canvas）
      const bg = cs.backgroundColor
      typo = {
        family: cs.fontFamily, size: cs.fontSize, line: cs.lineHeight,
        fg: cs.color,
        bg: bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent' ? bg : '',
      }
    })
    return () => cancelAnimationFrame(raf)
  })

  let roots = $derived.by(() => { void outline.version; return childrenOf(outline.tree, null) })

  // 搜索：过滤当前文档大纲。visibleIds 非 null 时仅保留命中节点及其祖先路径。
  let searchOpen = $state(false)
  let searchQuery = $state('')
  let searchInputEl: HTMLInputElement | undefined = $state()
  let visibleIds = $derived.by<Set<string> | null>(() => {
    void outline.version
    const q = searchQuery.trim().toLowerCase()
    if (!q) return null
    const nodes = outline.tree.nodes
    const set = new Set<string>()
    for (const n of nodes.values()) {
      if (!n.content.toLowerCase().includes(q)) continue
      let cur: NodeT | undefined = n
      while (cur && !set.has(cur.id)) { set.add(cur.id); cur = cur.parentId ? nodes.get(cur.parentId) : undefined }
    }
    return set
  })
  // 聚焦(zoom)优先于搜索:聚焦时只渲染 focus 根这一棵子树(含根本身)。
  let visibleRoots = $derived.by(() => {
    void outline.version
    const f = effectiveFocus
    if (f) {
      const n = outline.tree.nodes.get(f)
      return n ? [n] : []            // 失效 id → 空,由下方 effect 回退到全文
    }
    return visibleIds ? roots.filter((r) => visibleIds!.has(r.id)) : roots
  })
  // 失效保护:聚焦节点被删/不存在 → 回退全文(受控则通知宿主清空)。
  $effect(() => {
    void outline.version
    const f = effectiveFocus
    if (f && !outline.tree.nodes.get(f)) untrack(() => focusNode(null))
  })

  function toggleSearch() {
    searchOpen = !searchOpen
    if (!searchOpen) searchQuery = ''
    else queueMicrotask(() => searchInputEl?.focus())
  }
  function onSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); searchQuery = ''; searchOpen = false }
  }

  function onDragOp(drag: string, target: string, mode: 'sibling' | 'child') {
    const ids = drag.split(',')
    const ok = ids.length > 1
      ? (mode === 'child' ? moveNodesToChild(outline.tree, new Set(ids), target) : moveNodesAfter(outline.tree, new Set(ids), target))
      : (mode === 'child' ? moveNodeToChild(outline.tree, drag, target) : moveNodeAfter(outline.tree, drag, target))
    if (ok) { clearSelection(); bump(); markDirty() }
  }
  function addRootNote() {
    const last = roots[roots.length - 1]
    const node: NodeT = {
      id: newId(), parentId: null, order: calculateOrderBetween(last ? last.order : null, null),
      content: '', collapsed: false, source: 'manual',
    }
    outline.tree.nodes.set(node.id, node)
    outline.editingId = node.id
    bump()   // no markDirty(): an empty node alone must not trigger a save
  }

  // Click in the empty region below the last node → new trailing root node.
  // 拖拽手势（mousedown/mouseup 落点不同）的 click 会被浏览器派发到共同祖先
  // ——常常就是这个空白容器；只有"按下也在空白处且未形成框选"的完整点击才建节点。
  function onBodyClick(e: MouseEvent) {
    const target = e.target as HTMLElement
    if (target.closest('.node')) return   // clicks on existing rows handled by the node
    if (bandJustEnded) { bandJustEnded = false; return }  // 框选收尾的 click 不建节点
    const wasEmptyClick = emptyClickOk
    emptyClickOk = false
    if (outline.selectedIds.size > 0) { clearSelection(); return }  // 有选择时点空白只清除
    if (wasEmptyClick) addRootNote()
  }

  // Default-editable: an empty outline gets one ready-to-type root node
  // (no + button needed). Guarded so it fires once, not on every bump.
  // docPath === notePath gate: the global tree is a singleton — during an
  // attach/detach transition it can momentarily be empty while belonging to
  // another doc (or nothing). Auto-creating a node then would let that phantom
  // empty node get serialized over the real note. Only seed when the tree is
  // genuinely THIS note's, fully attached.
  $effect(() => {
    void outline.version
    if (outline.docPath === notePath && outline.tree.nodes.size === 0 && outline.editingId == null) addRootNote()
  })
  // Close any floating menu whose owning node is no longer in edit mode (e.g. blur → commitEdit).
  $effect(() => {
    if (menu.kind !== 'none' && outline.editingId !== menu.nodeId) {
      menu = { kind: 'none' }
    }
  })

  // ---------- 多选：框选 + 批量快捷键 ----------
  let bodyEl = $state<HTMLDivElement>()
  let band = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null)
  let bandStart: { x: number; y: number } | null = null
  let bandJustEnded = false
  let emptyClickOk = false   // 本次手势从空白处按下且未成为框选 → 允许建节点

  function bandRect(b: NonNullable<typeof band>) {
    return {
      left: Math.min(b.x0, b.x1), top: Math.min(b.y0, b.y1),
      width: Math.abs(b.x1 - b.x0), height: Math.abs(b.y1 - b.y0),
    }
  }
  // 行内文字上按下：先允许原生选字；拖过节点边界后切换为跨节点多选
  let rowDragFrom: { id: string; top: number; bottom: number } | null = null
  let crossDrag = false

  function nodeIdAt(x: number, y: number): string | null {
    const el = document.elementFromPoint(x, y) as HTMLElement | null
    return el?.closest<HTMLElement>('[data-node-id]')?.dataset.nodeId ?? null
  }
  function onBandDown(e: PointerEvent) {
    if (e.button !== 0) return
    const rowEl = (e.target as HTMLElement).closest<HTMLElement>('[data-node-id]')
    if (rowEl) {
      // 行上按下：不阻止默认（节点内选字），记录起点行的几何边界等待跨行
      const r = rowEl.getBoundingClientRect()
      rowDragFrom = { id: rowEl.dataset.nodeId!, top: r.top, bottom: r.bottom }
      crossDrag = false
      return
    }
    // 空白处按下：取消 mousedown 默认行为，不启动原生文字选区
    e.preventDefault()
    window.getSelection()?.removeAllRanges()
    bandStart = { x: e.clientX, y: e.clientY }
    bodyEl?.setPointerCapture(e.pointerId)
  }
  function onBandMove(e: PointerEvent) {
    if (rowDragFrom) {
      // 用起始行的几何边界（±2px 容差）判定跨行，elementFromPoint 的像素漂移
      // 会把行内选字误判成跨节点多选
      if (!crossDrag && e.clientY >= rowDragFrom.top - 2 && e.clientY <= rowDragFrom.bottom + 2) return
      if (!crossDrag) {
        crossDrag = true
        ;(document.activeElement as HTMLElement | null)?.blur?.()
        outline.editingId = null
      }
      window.getSelection()?.removeAllRanges()
      const over = nodeIdAt(e.clientX, e.clientY)
      if (over) setSelection(rangeBetween(outline.tree, rowDragFrom.id, over))
      return
    }
    if (!bandStart) return
    // 4px 阈值内仍算普通点击（保留点空白建节点）
    if (!band && Math.hypot(e.clientX - bandStart.x, e.clientY - bandStart.y) < 4) return
    if (!band) {
      // 框选一生效就先退出编辑并主动失焦——若拖到松手时才卸载聚焦中的
      // textarea，浏览器会把焦点/滚动跳回原编辑位置
      ;(document.activeElement as HTMLElement | null)?.blur?.()
      outline.editingId = null
    }
    // 指针拖出面板时把矩形夹取在列表区域内，不画到标题栏/其他 UI 上
    const bounds = bodyEl!.getBoundingClientRect()
    const cx = Math.min(Math.max(e.clientX, bounds.left), bounds.right)
    const cy = Math.min(Math.max(e.clientY, bounds.top), bounds.bottom)
    band = { x0: bandStart.x, y0: bandStart.y, x1: cx, y1: cy }
    const r = bandRect(band)
    const hit: string[] = []
    for (const el of bodyEl?.querySelectorAll<HTMLElement>('[data-node-id]') ?? []) {
      const b = el.getBoundingClientRect()
      if (b.left < r.left + r.width && b.left + b.width > r.left && b.top < r.top + r.height && b.top + b.height > r.top) {
        hit.push(el.dataset.nodeId!)
      }
    }
    setSelection(hit)
  }
  function onBandUp(e: PointerEvent) {
    if (rowDragFrom) {
      if (crossDrag) {
        // 跨节点多选收尾：吞掉随后的 click，防止松手处的行进入编辑
        const swallow = (ce: MouseEvent) => { ce.stopPropagation(); ce.preventDefault() }
        window.addEventListener('click', swallow, { capture: true, once: true })
        requestAnimationFrame(() => window.removeEventListener('click', swallow, { capture: true }))
      }
      rowDragFrom = null
      crossDrag = false
      return
    }
    if (!bandStart) return
    bodyEl?.releasePointerCapture(e.pointerId)
    if (band) {
      bandJustEnded = true
      // 松手点若落在某行文字上，随后的 click 会命中该行 span → startEdit →
      // 焦点+滚动跳到该节点。捕获阶段一次性吞掉这次 click。
      const swallow = (ce: MouseEvent) => { ce.stopPropagation(); ce.preventDefault() }
      window.addEventListener('click', swallow, { capture: true, once: true })
      // 下一帧兜底：这次手势没产生 click 时摘掉监听并复位 flag，别吞下次点击
      requestAnimationFrame(() => {
        window.removeEventListener('click', swallow, { capture: true })
        bandJustEnded = false
      })
    } else {
      emptyClickOk = true
      requestAnimationFrame(() => (emptyClickOk = false))
    }
    bandStart = null
    band = null
  }

  async function onGlobalKeydown(e: KeyboardEvent) {
    if (outline.selectedIds.size === 0 || outline.editingId != null) return
    // 其它输入焦点（主编辑器/搜索框）时不抢键
    const ae = document.activeElement
    if (ae && (ae.tagName === 'TEXTAREA' || ae.tagName === 'INPUT' || (ae as HTMLElement).isContentEditable)) return
    if (e.key === 'Escape') { e.preventDefault(); clearSelection(); return }
    if (e.key === 'Backspace' || e.key === 'Delete') {
      e.preventDefault()
      const roots = selectionRoots(outline.tree, outline.selectedIds)
      const hasKids = roots.some(n => childrenOf(outline.tree, n.id).length > 0)
      if (hasKids) {
        const { confirm } = await import('@tauri-apps/plugin-dialog')
        if (!await confirm(t('outline.deleteConfirm'), { title: t('outline.delete') })) return
      }
      if (deleteNodes(outline.tree, outline.selectedIds)) { clearSelection(); bump(); markDirty() }
      return
    }
    if (e.key === 'Tab') {
      e.preventDefault()
      const changed = e.shiftKey
        ? outdentNodes(outline.tree, outline.selectedIds)
        : indentNodes(outline.tree, outline.selectedIds)
      if (changed) { bump(); markDirty() }
      return
    }
    if (e.key === 'c' && (e.metaKey || e.ctrlKey)) {
      const md = nodesToMarkdown(outline.tree, outline.selectedIds)
      if (md) {
        e.preventDefault()
        const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
        await writeText(md)
      }
    }
  }
  // 保存按钮脏态：笔记以 tab 打开 → 跟随 tab 脏；否则跟随 panel-disk 的 outline.dirty
  let saveDirty = $derived(noteTab ? noteTab.currentContent !== noteTab.initialContent : outline.dirty)
  async function onSave() {
    if (noteTab) {
      const { saveTab } = await import('../../lib/tabs.svelte')
      await saveTab(noteTab.id)
    } else {
      await flushDisk()
    }
  }
  /** 主文档当前文本(用于重建时的 auto 派生) */
  function currentMainContent(): string | null {
    return mainTab ? mainTab.currentContent
      : tabs.find(x => x.filePath === mainPath)?.currentContent ?? null
  }

  async function reloadRemote() {
    const diskText = outline.externalConflict?.diskText ?? ''
    outline.externalConflict = null
    noteDiskHash = await sha256Hex(diskText).catch(() => null)
    const restore = captureScroll(bodyEl)
    await attachDoc(notePath, diskText, currentMainContent())   // 重置 dirty=false、armed 随内容
    setAnswersFromText(notePath, diskText)   // 正文答复卡片吃同一份文本,不会与大纲不一致
    restore()
  }

  /**
   * 伴生 .note.md 被外部改动(别的 agent、或别的设备经 git 同步)后的处理。
   * 既有 file-watcher 只盯 tab.filePath,覆盖不到 panel 模式下的伴生笔记。
   * 干净就静默重载、脏则交冲突横幅 —— 与主文档的策略一致,绝不静默盖掉未保存内容。
   */
  async function onCompanionChanged(path: string) {
    if (outline.docPath !== path) return       // 期间切走了文档
    if (outline.externalConflict) return       // 已有未解决冲突,别再叠加
    try {
      const fs = await import('@tauri-apps/plugin-fs')
      const diskText = await fs.readTextFile(path).catch(() => null)
      if (diskText == null) return
      const diskHash = await sha256Hex(diskText).catch(() => null)
      if (diskHash == null) return
      // 用「内存树序列化后的 hash」判定本地有没有会被覆盖的东西,而不是 outline.dirty ——
      // 后者会被派生同步(markSynced)置真,主文档随便敲几个字就会把正常重载误判成冲突。
      const ourHash = await sha256Hex(serializeDoc(false)).catch(() => null)
      if (ourHash != null && ourHash === diskHash) { noteDiskHash = diskHash; return } // 已与磁盘一致
      const dirty = ourHash != null && noteDiskHash != null ? ourHash !== noteDiskHash : outline.dirty
      const decision = decideCompanionReload({ diskHash, lastHash: noteDiskHash, dirty })
      if (decision === 'ignore') return        // 自写回声
      if (decision === 'conflict') { outline.externalConflict = { diskText }; return }
      if (outline.docPath !== path) return     // 读盘期间切走了
      const restore = captureScroll(bodyEl)
      noteDiskHash = diskHash
      await attachDoc(path, diskText, currentMainContent())
      // 只在这份笔记正是当前正文所对应的那份时才刷新索引,避免串台
      if (answersStore.notePath === path) setAnswersFromText(path, diskText)
      restore()
    } catch (e) {
      console.warn('[outline] companion reload failed:', e)
    }
  }

  /**
   * 装伴生笔记监听 —— **仅 panel-disk 模式**(伴生笔记没有被开成 tab)。
   *
   * 笔记一旦有 tab 撑着,外部变更就归既有 file-watcher 管:它有正确的
   * `lastKnownHash` 基线与 `recordOurWrite` 配套,而本组件的 `noteDiskHash`
   * 只在 panel-disk 写盘路径上维护。两边都插手会把自己的保存误判成外部冲突。
   * tab 那条路重载后派发 `mdeditor:auto-reloaded`,上面的监听器负责重建树。
   *
   * 监听的是**父目录**而非文件本身:笔记可能尚未存在(agent 稍后才创建)、
   * 也可能被 git 同步以「删除+重建」或改名的方式替换 —— 盯文件会一装就失败、
   * 或在替换后失效。
   */
  $effect(() => {
    const path = notePath
    if (!path || noteTab) return
    const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
    if (slash <= 0) return
    const dir = path.slice(0, slash)
    const base = path.slice(slash + 1)
    let stop: (() => void) | null = null
    let timer: ReturnType<typeof setTimeout> | null = null
    let disposed = false
    void (async () => {
      try {
        const { watchImmediate } = await import('@tauri-apps/plugin-fs')
        const s = await watchImmediate(dir, (event) => {
          const paths = (event as { paths?: string[] }).paths
          // 事件形状异常时不过滤,交给 onCompanionChanged 的 hash 比对兜底(幂等)
          if (Array.isArray(paths) && paths.length && !paths.some(p => p.endsWith(base))) return
          if (timer) clearTimeout(timer)
          timer = setTimeout(() => { void onCompanionChanged(path) }, 300)   // 合并抖动
        }, { recursive: false })
        if (disposed) { try { s() } catch { /* 已卸载 */ } } else stop = s
      } catch {
        // 目录不可监听(网络盘/沙盒)→ 静默降级,切换文档时仍会重新加载
      }
    })()
    return () => {
      disposed = true
      if (timer) clearTimeout(timer)
      if (stop) { try { stop() } catch { /* watcher 已失效 */ } }
    }
  })
  async function overwriteLocal() {
    outline.externalConflict = null
    const text = serializeDoc()
    const fs = await import('@tauri-apps/plugin-fs')
    await fs.writeTextFile(notePath, text)
    noteDiskHash = await sha256Hex(text).catch(() => null)
    outline.armed = true
    markSaved()
  }
  async function onRegenerate() {
    const { confirm } = await import('@tauri-apps/plugin-dialog')
    if (!(await confirm(t('outline.regenerateConfirm'), { title: t('outline.regenerate') }))) return
    let mainContent = tabs.find(x => x.filePath === mainPath)?.currentContent
    if (mainContent == null) {
      const { exists, readTextFile } = await import('@tauri-apps/plugin-fs')
      if (await exists(mainPath).catch(() => false)) {
        mainContent = await readTextFile(mainPath).catch(() => undefined)
      }
    }
    if (mainContent == null) return
    regenerate(mainContent)
  }
  type MenuState =
    | { kind: 'none' }
    | { kind: 'slash'; nodeId: string; start: number; query: string; selected: number; x: number; y: number; el: HTMLTextAreaElement }
    | { kind: 'link'; nodeId: string; start: number; query: string; selected: number; x: number; y: number; el: HTMLTextAreaElement }
  let menu = $state<MenuState>({ kind: 'none' })

  let slashItems = $derived(menu.kind === 'slash' ? filterSlashItems(menu.query) : [])
  let linkPages = $derived(menu.kind === 'link'
    ? filterPages(outline.backlinkIndex ? pageCandidates(outline.backlinkIndex) : [], menu.query)
    : [])

  function menuAnchor(el: HTMLTextAreaElement): { x: number; y: number } {
    const r = el.getBoundingClientRect()
    return { x: Math.min(r.left, window.innerWidth - 220), y: r.bottom + 2 }
  }

  // Invariant: el.value and node.content must be set to the SAME string before bump() so
  // Svelte's `value={node.content}` attribute reconciliation skips the DOM write and the
  // caret position set by setSelectionRange survives without being reset by a Svelte patch.
  function applyToTextarea(el: HTMLTextAreaElement, node: NodeT, text: string, cursor: number) {
    el.value = text
    setNodeContent(node, text)
    el.setSelectionRange(cursor, cursor)
    bump(); markDirty()
  }

  /** 返回 true = 事件被菜单消费（keydown 时 Node 组件会 preventDefault） */
  function onEditorInput(node: NodeT, value: string, cursor: number, el: HTMLTextAreaElement, e?: KeyboardEvent): boolean {
    // --- keydown 阶段：菜单打开时接管导航键 ---
    if (e && menu.kind !== 'none') {
      const count = menu.kind === 'slash' ? slashItems.length : linkPages.length
      if (e.key === 'ArrowDown') { menu.selected = (menu.selected + 1) % Math.max(count, 1); return true }
      if (e.key === 'ArrowUp') { menu.selected = (menu.selected - 1 + Math.max(count, 1)) % Math.max(count, 1); return true }
      if (e.key === 'Escape') { menu = { kind: 'none' }; return true }
      if (e.key === 'Enter') {
        if (menu.kind === 'slash' && slashItems[menu.selected]) { pickSlash(slashItems[menu.selected]); return true }
        if (menu.kind === 'link') { pickPage(linkPages[menu.selected] ?? null); return true }
        menu = { kind: 'none' }
        return false
      }
      return false
    }
    if (e) {
      // `[` 后接着输 `[` → 自动补 `]]` 并开链接菜单（render.cljs:1117）
      if (e.key === '[' && value[cursor - 1] === '[') {
        const text = value.slice(0, cursor) + '[]]' + value.slice(cursor)
        applyToTextarea(el, node, text, cursor + 1)
        menu = { kind: 'link', nodeId: node.id, start: cursor - 1, query: '', selected: 0, ...menuAnchor(el), el }
        return true
      }
      return false
    }
    // --- input 阶段：维护菜单 query / 触发 slash 菜单 ---
    if (menu.kind === 'link') {
      const q = pageLinkQueryAt(value, cursor)
      if (q && q.start === menu.start) menu.query = q.query
      else menu = { kind: 'none' }
      return false
    }
    if (menu.kind === 'slash') {
      const seg = value.slice(menu.start, cursor)
      if (seg.startsWith('/') && !seg.includes(' ')) { menu.query = seg.slice(1); menu.selected = 0 }
      else menu = { kind: 'none' }
      return false
    }
    // `/` 在行首或空格后触发（render.cljs filtered-slash-commands 语义）
    if (value[cursor - 1] === '/' && (cursor === 1 || /\s/.test(value[cursor - 2]))) {
      menu = { kind: 'slash', nodeId: node.id, start: cursor - 1, query: '', selected: 0, ...menuAnchor(el), el }
    }
    return false
  }

  function pickSlash(item: SlashItem) {
    if (menu.kind !== 'slash') return
    const node = outline.tree.nodes.get(menu.nodeId)
    if (node) {
      const r = applySlashItem(menu.el.value, menu.start, menu.el.selectionStart, item)
      applyToTextarea(menu.el, node, r.text, r.cursor)
    }
    menu = { kind: 'none' }
  }

  function pickPage(page: string | null) {
    if (menu.kind !== 'link') return
    const node = outline.tree.nodes.get(menu.nodeId)
    if (node) {
      const r = confirmPageLink(menu.el.value, menu.start, menu.query, page)
      applyToTextarea(menu.el, node, r.text, r.cursor)
    }
    menu = { kind: 'none' }
  }

  let ctxMenu = $state<{ node: NodeT; x: number; y: number } | null>(null)

  function onContextMenu(e: MouseEvent, node: NodeT) {
    ctxMenu = { node, x: e.clientX, y: e.clientY }
  }
  async function onCtxAction(action: string, node: NodeT) {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
    if (action === 'jump') onJump(node)
    else if (action === 'copy') await writeText(node.content)
    else if (action === 'copy-subtree') await writeText(subtreeToMarkdown(outline.tree, node.id))
    else if (action === 'copy-ref') { pinnedIds.add(node.id); await writeText(`((${node.id}))`); markDirty() }
    else if (action === 'delete') {
      const { confirm } = await import('@tauri-apps/plugin-dialog')
      const kids = childrenOf(outline.tree, node.id).length
      if (kids === 0 || await confirm(t('outline.deleteConfirm'), { title: t('outline.delete') })) {
        if (deleteNode(outline.tree, node.id)) { bump(); markDirty() }
      }
    }
  }
</script>

<div class="outline-editor" class:embedded oncontextmenu={(e) => { if (!embedded) e.preventDefault() }}
  style={embedded ? '' : `--outline-font-family: ${typo.family}; --outline-font-size: ${typo.size}; --outline-line-height: ${typo.line};${typo.fg ? ` color: ${typo.fg};` : ''}${typo.bg ? ` background: ${typo.bg};` : ''}`}>
  <div class="typo-probe" data-theme={activeThemeId} aria-hidden="true" bind:this={probeEl}>
    <div class="moraya-editor"></div>
  </div>
  {#if !embedded}
  <div class="toolbar">
    <span class="doc-title">{pageNameOf(notePath)}</span>
    <button class="hbtn" class:on={searchOpen} title={t('outline.search')} aria-label={t('outline.search')} onclick={toggleSearch}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="11" cy="11" r="8" />
        <line x1="21" y1="21" x2="16.65" y2="16.65" />
      </svg>
    </button>
    <button class="hbtn" title={t('outline.regenerate')} aria-label={t('outline.regenerate')} onclick={onRegenerate}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <polyline points="23 4 23 10 17 10" />
        <polyline points="1 20 1 14 7 14" />
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
      </svg>
    </button>
    <button class="hbtn" class:dirty={saveDirty} title={t('outline.save')} aria-label={t('outline.save')} disabled={!saveDirty} onclick={() => void onSave()}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2Z" />
        <polyline points="17 21 17 13 7 13 7 21" />
        <polyline points="7 3 7 8 15 8" />
      </svg>
    </button>
  </div>
  {/if}
  {#if outline.externalConflict}
    <div class="conflict-banner" role="alert">
      <span class="conflict-msg">{t('outline.externalChanged')}</span>
      <button class="conflict-btn" onclick={() => void reloadRemote()}>{t('externalChange.reload')}</button>
      <button class="conflict-btn" onclick={() => void overwriteLocal()}>{t('externalChange.overwrite')}</button>
    </div>
  {/if}
  {#if searchOpen}
    <div class="search-row">
      <input
        bind:this={searchInputEl}
        class="search-input"
        type="text"
        placeholder={t('outline.searchPlaceholder')}
        bind:value={searchQuery}
        onkeydown={onSearchKeydown}
      />
      {#if searchQuery}
        <button class="hbtn" title={t('common.close')} aria-label={t('common.close')} onclick={() => (searchQuery = '')}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      {/if}
    </div>
  {/if}
  {#if effectiveFocus && !focusControlled && outline.tree.nodes.get(effectiveFocus)}
    <!-- 非受控就地 zoom 自带面包屑;受控(daily 聚焦视图)由宿主渲染带日期的面包屑。 -->
    <OutlineBreadcrumb tree={outline.tree} focusRootId={effectiveFocus} rootLabel={pageNameOf(notePath)} onCrumb={focusNode} />
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="body" role="tree" bind:this={bodyEl} onclick={onBodyClick}
    onpointerdown={onBandDown} onpointermove={onBandMove} onpointerup={onBandUp}>
    {#each visibleRoots as node (node.id)}
      <OutlineNode {node} depth={0} {resolved} {onJump} {onPageClick} {onEditorInput} {onContextMenu} {onDragOp} {visibleIds} {onCollapse} onFocus={(n) => focusNode(n.id)} forceExpand={effectiveFocus === node.id} />
    {/each}
    {#if visibleRoots.length === 0}
      <p class="empty">{visibleIds ? t('outline.noSearchResults') : t('outline.empty')}</p>
    {/if}
    {#if !visibleIds && showLinkedRefs}
      <!-- Rendered inside the scroll body so it reads as one continuous outline.
           Stop pointer/click from bubbling to the outline's band-select / body-click.
           Hidden in the Daily Notes feed; shown in the single-page view. -->
      <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
      <div class="lr-wrap" onpointerdown={(e) => e.stopPropagation()} onclick={(e) => e.stopPropagation()}>
        <LinkedReferences page={pageNameOf(notePath)} excludeFile={notePath} />
      </div>
    {/if}
  </div>
  {#if menu.kind === 'slash'}
    <SlashMenu items={slashItems} selected={menu.selected} x={menu.x} y={menu.y} onPick={pickSlash} />
  {:else if menu.kind === 'link'}
    <LinkAutocomplete pages={linkPages} selected={menu.selected} x={menu.x} y={menu.y} onPick={pickPage} />
  {/if}
  {#if ctxMenu}
    <NodeContextMenu node={ctxMenu.node} x={ctxMenu.x} y={ctxMenu.y} onAction={onCtxAction} onClose={() => (ctxMenu = null)} />
  {/if}
  {#if band}
    {@const r = bandRect(band)}
    <div class="band" style="left:{r.left}px; top:{r.top}px; width:{r.width}px; height:{r.height}px"></div>
  {/if}
</div>

<svelte:window onkeydown={onGlobalKeydown} />

<style>
  .outline-editor {
    flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column;
    /* 整个编辑器永久禁用原生文字选区/拖字——多选由大纲交互自己处理；
       编辑态 textarea 与搜索框内例外 */
    user-select: none;
    -webkit-user-select: none;
  }
  .outline-editor :global(textarea), .outline-editor :global(input) {
    user-select: text;
    -webkit-user-select: text;
  }
  .toolbar {
    display: flex; align-items: center; gap: 4px; padding: 6px 16px;
    border-bottom: 1px solid var(--border-color, #3333);
  }
  .doc-title { flex: 1; font-size: 13px; font-weight: 600; opacity: 0.75; }
  .hbtn {
    display: inline-flex; align-items: center; justify-content: center;
    border: 0; background: transparent; cursor: pointer;
    padding: 3px; border-radius: 4px; opacity: 0.7;
  }
  .hbtn svg { display: block; }
  .hbtn:hover:not(:disabled) { background: rgba(0,0,0,0.08); opacity: 1; }
  .hbtn:disabled { opacity: 0.25; cursor: default; }
  .hbtn.on { background: rgba(0,0,0,0.1); opacity: 1; }
  .hbtn.dirty { position: relative; color: var(--accent-color, #4a80d4); opacity: 1; }
  .hbtn.dirty::after {
    content: ''; position: absolute; top: 1px; right: 1px;
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent-color, #4a80d4);
  }
  .search-row {
    display: flex; align-items: center; gap: 4px;
    padding: 4px 8px; border-bottom: 1px solid var(--border-color, #3333);
  }
  .search-input {
    flex: 1; min-width: 0; font-size: 12px; padding: 3px 6px;
    border: 1px solid var(--border-color, #3335); border-radius: 4px;
    background: var(--input-bg, transparent); color: inherit; outline: none;
  }
  .search-input:focus { outline: 1px solid var(--accent-color, #4a80d4); }
  .body {
    flex: 1; overflow-y: auto; padding: 16px 24px; max-width: 860px; width: 100%;
    margin: 0 auto; box-sizing: border-box; font-family: var(--outline-font-family);
  }
  /* Embedded (Daily Notes): no outer padding so the outline aligns with the
     read-only view and days flow continuously. */
  .outline-editor.embedded .body { padding: 0; }
  /* 低调的中性框选矩形（Finder 风格），跟随主题前景色 */
  .band {
    position: fixed; z-index: 30; pointer-events: none;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    background: color-mix(in srgb, currentColor 6%, transparent);
    border-radius: 2px;
  }
  .empty { opacity: 0.5; font-size: 12px; }
  .conflict-banner {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    padding: 6px 16px; font-size: 12px;
    background: color-mix(in srgb, #e0a030 18%, transparent);
    border-bottom: 1px solid color-mix(in srgb, #e0a030 40%, transparent);
  }
  .conflict-msg { flex: 1; min-width: 0; opacity: 0.9; }
  .conflict-btn {
    border: 1px solid var(--border-color, #3335); background: transparent;
    color: inherit; cursor: pointer; padding: 2px 8px; border-radius: 4px; font-size: 12px;
  }
  .conflict-btn:hover { background: rgba(0,0,0,0.08); }
  .typo-probe {
    position: absolute;
    left: -9999px; top: 0;
    width: 0; height: 0;
    visibility: hidden;
    pointer-events: none;
  }
  @media (prefers-color-scheme: dark) {
    .hbtn:hover:not(:disabled) { background: rgba(255,255,255,0.1); }
    .hbtn.on { background: rgba(255,255,255,0.15); }
    .conflict-btn:hover { background: rgba(255,255,255,0.1); }
  }
</style>
