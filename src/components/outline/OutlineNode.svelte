<script lang="ts">
  import OutlineNode from './OutlineNode.svelte'
  import InlineRender from './InlineRender.svelte'
  import { outline, bump, markDirty, setSelection, clearSelection } from '../../lib/outline/store.svelte'
  import { rangeBetween, selectionRoots } from '../../lib/outline/select'
  import { childrenOf, setNodeContent, type OutlineNode as NodeT, type OutlineTree } from '../../lib/outline/model'
  import {
    createSiblingBelow, createSiblingAbove, mergeWithPrevious,
    indentNode, outdentNode, moveNodeUp, moveNodeDown, applyInlineWrap,
    insertPastedTree,
  } from '../../lib/outline/commands'
  import { parseClipboardOutline } from '../../lib/outline/paste'
  import { visibleNodes } from '../../lib/outline/model'
  import { matchCommand, type OutlineCommandId } from '../../lib/outline/shortcuts'
  import { writeBackNoteEdit } from '../../lib/outline/note-writeback-io'
  import { t } from '../../lib/i18n/store.svelte'

  let {
    node, depth, resolved = {} as Record<OutlineCommandId, string>,
    onJump = () => {}, onPageClick,
    onEditorInput = () => false, onContextMenu = () => {}, onDragOp = () => {},
    visibleIds = null,
    readonly = false, tree = null, onActivate = null, onCollapse = null, foldVersion = 0,
    onFocus = null, forceExpand = false,
  }: {
    node: NodeT
    depth: number
    resolved?: Record<OutlineCommandId, string>
    onJump?: (n: NodeT) => void
    onPageClick: (target: string) => void
    /** 编辑态每次 input：内容、光标、textarea 元素（菜单锚定用，Task 13） */
    onEditorInput?: (node: NodeT, value: string, cursor: number, el: HTMLTextAreaElement, e?: KeyboardEvent) => boolean
    onContextMenu?: (e: MouseEvent, n: NodeT) => void
    onDragOp?: (drag: string, target: string, mode: 'sibling' | 'child') => void
    /** 搜索过滤：非 null 时仅渲染集合内的子节点（保住匹配节点的祖先路径），并无视折叠展开命中项 */
    visibleIds?: Set<string> | null
    /** 只读模式（每日笔记非激活天）：不编辑/不拖拽/不选择，仅渲染 + 折叠 + 双链 +
     *  点击请求激活。从传入的 `tree` 读子节点，不依赖全局 outline 单例。 */
    readonly?: boolean
    /** 只读模式下用于读取子节点的树（$state，深响应）；null=用全局 outline.tree。 */
    tree?: OutlineTree | null
    /** 只读模式：点击节点内容请求把该天切成可编辑并定位到此节点。 */
    onActivate?: ((n: NodeT) => void) | null
    /** 只读模式：折叠状态变更后回调（持久化折叠记忆；参数为被切换的节点）。 */
    onCollapse?: ((n: NodeT) => void) | null
    /** 只读模式重渲染信号：折叠切换时父级 bump 它，强制折叠相关 derive 重算
     *  （只读树在 outline 单例之外，node.collapsed 变异经 Map 代理不一定响应）。 */
    foldVersion?: number
    /** bullet 点击 = zoom-in 到该节点(Roam/hulunote 语义);null=不支持聚焦。
     *  折叠交给 tri 三角,bullet 只发聚焦事件,由宿主决定就地/切视图。 */
    onFocus?: ((n: NodeT) => void) | null
    /** 该行是聚焦视图的顶行(focus 根):忽略自身 collapsed,始终展开其直接子节点,
     *  这样聚焦一个折叠节点也能看到内容。仅作用于顶行,子节点各自遵守 collapsed。 */
    forceExpand?: boolean
  } = $props()

  /** note/question 同族:annotation 的批注文本子节点(question=含问号升格) */
  const noteLike = (s: NodeT['source']) => s === 'note' || s === 'question'

  const srcTree = $derived(tree ?? outline.tree)
  let kids = $derived.by(() => {
    if (!readonly) void outline.version
    const all = childrenOf(srcTree, node.id)
    return visibleIds ? all.filter((k) => visibleIds.has(k.id)) : all
  })
  let editing = $derived(!readonly && outline.editingId === node.id)
  // Route collapse state through outline.version so bump() re-renders it —
  // `node.collapsed` is a plain (non-proxied) property Svelte doesn't track.
  // In readonly mode the tree is a $state proxy, so node.collapsed is reactive
  // on its own (no version needed).
  let isCollapsed = $derived.by(() => { if (readonly) void foldVersion; else void outline.version; return node.collapsed })
  // Same for content: sync/note-writeback mutate `node.content` in place on
  // the same plain object, so a bump() must re-read it or the row keeps
  // showing stale text until something else re-renders it (e.g. a click).
  let content = $derived.by(() => { if (!readonly) void outline.version; return node.content })
  let showChildren = $derived.by(() => {
    if (readonly) void foldVersion; else void outline.version
    if (forceExpand) return kids.length > 0            // 聚焦顶行:无视 collapsed
    return visibleIds ? kids.length > 0 : !node.collapsed
  })
  let textareaEl: HTMLTextAreaElement | undefined = $state()
  let selected = $derived(!readonly && outline.selectedIds.has(node.id))
  // note/question 子节点可编辑（其余 auto 只读）；编辑起点内容留作回写定位的"旧批注"
  let editable = $derived(node.source === 'manual' || noteLike(node.source))
  // 批注行（被批注的原文/※ 占位符）：样式如高亮（金色下划线）
  let markLike = $derived(node.source === 'annotation')
  let noteBaseline: string | null = null

  $effect(() => {
    if (editing && noteLike(node.source)) noteBaseline = node.content
  })

  $effect(() => {
    if (editing && textareaEl) {
      // rows=1 只在 oninput 时自适应；进入编辑态先按内容撑开，多行内容不塌成一行
      textareaEl.style.height = 'auto'
      textareaEl.style.height = textareaEl.scrollHeight + 'px'
      textareaEl.focus()
    }
  })

  function startEdit() {
    outline.editingId = node.id
    outline.selectionAnchor = node.id
  }
  /** 普通点击=清选择进编辑；Shift+点击=锚点连选（不进编辑） */
  function onContentClick(e: MouseEvent) {
    const anchor = outline.selectionAnchor ?? outline.editingId
    if (e.shiftKey && anchor && anchor !== node.id) {
      e.preventDefault()
      outline.editingId = null
      setSelection(rangeBetween(outline.tree, anchor, node.id))
      return
    }
    // 行内刚拖选了一段文字：保留文字选区（供复制），不进入编辑
    const sel = window.getSelection()
    if (sel && !sel.isCollapsed) return
    // 只读(auto)节点:内容不可编辑。绝不进入编辑态(否则只读节点会退化成手写节点)。
    // 有源位置 → 跳转定位(rich 按锚文本、source 按行号,见 onJump);
    // 无源位置(如从 .note.md 重载丢了 anchorLine)→ 不做特别处理。
    if (!editable) { if (node.anchorLine != null) onJump(node); return }
    clearSelection()
    startEdit()
  }
  // status 是普通属性,变更经 bump() 重读(同 content/collapsed 的处理)
  let qStatus = $derived.by(() => { if (!readonly) void outline.version; return node.status })

  /** chip 单击:open/answered → closed(裁决/误判清理);closed → open(重开) */
  function onChipClick() {
    node.status = node.status === 'closed' ? 'open' : 'closed'
    bump(); markDirty()
  }

  /** 追问自动重开:在 answered 问题下追加/修改 ● 手写内容 = 把问题拨回 open(spec §4)。
   *  覆盖三条提交路径:blur→commitEdit / Enter 键 / ArrowUp|Down 键。 */
  function reopenAnsweredAncestor() {
    const seen = new Set<string>()   // 环保护:坏树的 parentId 环不得死循环冻结 UI
    let pid = node.parentId
    while (pid && !seen.has(pid)) {
      seen.add(pid)
      const p = outline.tree.nodes.get(pid)
      if (!p) break
      if (p.source === 'question') {
        if (p.status === 'answered') p.status = 'open'
        break
      }
      pid = p.parentId
    }
  }

  function commitEdit(value: string) {
    if (noteLike(node.source)) {
      // 批注子节点：改动写回 .note.md 树 + 主文档的 {>>…<<}
      const old = noteBaseline ?? node.content
      outline.editingId = null
      if (value !== old) {
        setNodeContent(node, value)
        bump(); markDirty()
        void writeBackNoteEdit(node, old, value)
      }
      return
    }
    if (node.source !== 'manual') { outline.editingId = null; return }  // auto is read-only
    // 只在内容真的变了才置脏/激活自动保存：空的自动补根节点失焦、或点进点出没改动,
    // 都不该 arm 并落盘生成 .note.md（intent-save：写盘要有真实写笔记意愿）。
    const changed = value !== node.content
    setNodeContent(node, value)
    outline.editingId = null
    bump()
    if (changed) { reopenAnsweredAncestor(); markDirty() }
  }
  function onBulletClick() {
    // Roam/hulunote 语义：bullet 点击 = zoom-in 到该节点(折叠归 tri 三角)。
    // 宿主未接聚焦(onFocus=null)时,退回旧行为:可跳转的叶子跳到源。
    if (onFocus) { onFocus(node); return }
    if (node.anchorLine != null) onJump(node)
  }

  function focusNode(id: string | null) {
    outline.editingId = id
  }

  function onKeydown(e: KeyboardEvent) {
    const el = e.currentTarget as HTMLTextAreaElement
    // 先给菜单层机会（/ 与 [[ 菜单打开时接管 ↑↓/Enter/Esc）
    if (onEditorInput(node, el.value, el.selectionStart, el, e)) { e.preventDefault(); return }

    const atStart = el.selectionStart === 0 && el.selectionEnd === 0
    const atEnd = el.selectionStart === el.value.length

    if (e.key === 'Enter' && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault()
      if (noteLike(node.source)) {
        // 批注子节点：回车 = 提交并退出编辑
        commitEdit(el.value)
        return
      }
      if (node.source !== 'manual') {
        // read-only: Enter spawns an editable manual sibling directly below
        const id = createSiblingBelow(outline.tree, node.id)
        bump(); markDirty(); focusNode(id)
        return
      }
      if (el.value !== node.content) reopenAnsweredAncestor()
      setNodeContent(node, el.value)
      // 行首 Enter → 上方建兄弟（render.cljs handle-key-down 语义）
      const id = atStart && el.value.length > 0
        ? createSiblingAbove(outline.tree, node.id)
        : createSiblingBelow(outline.tree, node.id)
      bump(); markDirty(); focusNode(id)
      return
    }
    if (e.key === 'Backspace' && atStart) {
      if (noteLike(node.source)) return   // 批注子节点不参与合并
      const res = mergeWithPrevious(outline.tree, node.id)
      if (res) { e.preventDefault(); bump(); markDirty(); focusNode(res.mergedInto) }
      return
    }
    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.metaKey && !e.altKey) {
      const vis = visibleNodes(outline.tree)
      const idx = vis.findIndex(n => n.id === node.id)
      const nb = e.key === 'ArrowUp' ? (atStart ? vis[idx - 1] : null) : (atEnd ? vis[idx + 1] : null)
      if (nb) {
        e.preventDefault()
        if (noteLike(node.source)) commitEdit(el.value)
        else { if (el.value !== node.content) reopenAnsweredAncestor(); setNodeContent(node, el.value); bump(); markDirty() }
        focusNode(nb.source === 'manual' || noteLike(nb.source) ? nb.id : null)
      }
      return
    }
    const cmd = matchCommand(e, resolved)
    if (!cmd) return
    if (noteLike(node.source)) { e.preventDefault(); return }  // 结构命令对批注子节点无效
    e.preventDefault()
    setNodeContent(node, el.value)
    if (cmd === 'outline.indent') indentNode(outline.tree, node.id)
    else if (cmd === 'outline.outdent') outdentNode(outline.tree, node.id)
    else if (cmd === 'outline.toggleCollapse') node.collapsed = !node.collapsed
    else if (cmd === 'outline.moveUp') moveNodeUp(outline.tree, node.id)
    else if (cmd === 'outline.moveDown') moveNodeDown(outline.tree, node.id)
    else if ((cmd === 'outline.bold' || cmd === 'outline.italic') && node.source === 'manual') {
      const r = applyInlineWrap(el.value, el.selectionStart, el.selectionEnd, cmd === 'outline.bold' ? '**' : '__')
      el.value = r.text
      el.setSelectionRange(r.selStart, r.selEnd)
      setNodeContent(node, r.text)
    }
    bump(); markDirty()
  }

  function onPaste(e: ClipboardEvent) {
    if (node.source !== 'manual') return            // 只在手写节点上解析
    const cd = e.clipboardData
    if (!cd) return
    const text = cd.getData('text/plain')
    if (!text || !/\r|\n/.test(text)) return         // 单行/无文本 → 原生
    const el = e.currentTarget as HTMLTextAreaElement
    if (el.selectionStart !== el.selectionEnd) return // 有选区 → 原生
    const parsed = parseClipboardOutline(text)
    if (parsed.length < 2) return                     // 不构成层次 → 原生
    e.preventDefault()
    const head = el.value.slice(0, el.selectionStart)
    const tail = el.value.slice(el.selectionStart)
    el.value = head + parsed[0].content               // 同步 el.value，避免 blur 用旧值回写
    const lastId = insertPastedTree(outline.tree, node.id, head, tail, parsed)
    bump(); markDirty(); focusNode(lastId)
  }

  // 拖拽（render.cljs:733 detect-drop-mode：落点 X 在文本左缘右侧 → child）
  let dropMode: 'sibling' | 'child' | null = $state(null)
  function onDragStart(e: DragEvent) {
    if (node.source !== 'manual') { e.preventDefault(); return }
    // 拖动选中集内的节点 = 整组移动（roots 逗号列表；接收侧 split）
    const ids = selected
      ? selectionRoots(outline.tree, outline.selectedIds).filter(n => n.source === 'manual').map(n => n.id)
      : [node.id]
    e.dataTransfer?.setData('text/outline-node', ids.join(','))
  }
  function onDragOver(e: DragEvent) {
    if (!e.dataTransfer?.types.includes('text/outline-node')) return
    e.preventDefault()
    const row = e.currentTarget as HTMLElement
    const contentEl = row.querySelector('.content')
    const textLeft = contentEl?.getBoundingClientRect().left ?? 0
    dropMode = e.clientX >= textLeft ? 'child' : 'sibling'
  }
  function onDrop(e: DragEvent) {
    const dragId = e.dataTransfer?.getData('text/outline-node')
    if (dragId && dropMode) onDragOp(dragId, node.id, dropMode)
    dropMode = null
  }
</script>

<div class="node" class:has-guide={showChildren && kids.length > 0} style="--depth: {depth}">
  <div
    class="row"
    class:auto={node.source !== 'manual'}
    class:drop-sibling={dropMode === 'sibling'}
    class:drop-child={dropMode === 'child'}
    class:selected
    data-node-id={node.id}
    role="treeitem"
    aria-selected={editing || selected}
    ondragover={onDragOver}
    ondragleave={() => (dropMode = null)}
    ondrop={onDrop}
    oncontextmenu={(e) => { if (readonly) return; e.preventDefault(); onContextMenu(e, node) }}
  >
    {#if kids.length > 0}
      <button class="tri" class:closed={isCollapsed}
        onclick={() => {
          node.collapsed = !node.collapsed
          if (onCollapse) { if (!readonly) bump(); onCollapse(node) }
          else { bump(); markDirty() }
        }}>▾</button>
    {/if}
    <span
      class="bullet"
      class:has-kids={kids.length > 0}
      class:closed={kids.length > 0 && isCollapsed}
      class:src-toc={node.source === 'toc'}
      class:src-hl={node.source === 'highlight'}
      class:src-wl={node.source === 'wikilink'}
      class:src-anno={node.source === 'annotation'}
      class:src-note={noteLike(node.source)}
      class:jumpable={node.anchorLine != null}
      draggable={!readonly && node.source === 'manual'}
      ondragstart={onDragStart}
      onclick={onBulletClick}
    ></span>
    {#if editing}
      <textarea
        bind:this={textareaEl}
        class="content edit"
        class:hl={node.source === 'highlight' || markLike}
        class:src-toc={node.source === 'toc'}
        rows="1"
        value={content}
        onbeforeinput={(e) => { if (!editable) e.preventDefault() }}
        onblur={(e) => commitEdit((e.currentTarget as HTMLTextAreaElement).value)}
        onkeydown={onKeydown}
        onpaste={onPaste}
        oninput={(e) => {
          const el = e.currentTarget as HTMLTextAreaElement
          el.style.height = 'auto'
          el.style.height = el.scrollHeight + 'px'
          onEditorInput(node, el.value, el.selectionStart, el)
        }}
      ></textarea>
    {:else}
      <span class="content" class:hl={node.source === 'highlight' || markLike} class:src-toc={node.source === 'toc'}
        class:jump-src={!readonly && !editable && node.anchorLine != null}
        onclick={readonly ? () => onActivate?.(node) : onContentClick} role="button" tabindex="0"
        onkeydown={(e) => { if (e.key === 'Enter') { if (readonly) onActivate?.(node); else if (!editable) { if (node.anchorLine != null) onJump(node) } else startEdit() } }}>
        <!-- 空内容：塞零宽空格保证有行盒，鼠标可命中进入编辑 -->
        {#if content === ''}{'​'}{:else}<InlineRender {content} onPageClick={onPageClick} />{/if}
      </span>
    {/if}
    {#if !readonly && node.source === 'question'}
      <button class="qchip"
        class:st-answered={qStatus === 'answered'}
        class:st-closed={qStatus === 'closed'}
        title={t('outline.questionChip')}
        onclick={(e) => { e.stopPropagation(); onChipClick() }}
      >{qStatus ?? 'open'}</button>
    {/if}
  </div>
  {#if showChildren}
    {#each kids as child (child.id)}
      <OutlineNode node={child} depth={depth + 1} {resolved} {onJump} {onPageClick} {onEditorInput} {onContextMenu} {onDragOp} {visibleIds} {readonly} {tree} {onActivate} {onCollapse} {foldVersion} {onFocus} />
    {/each}
  {/if}
</div>

<style>
  /* font-size 挂到主题字号,使 .node 内的 em(缩进/沟槽/引导线)都随 theme 缩放 */
  .node { position: relative; font-size: var(--outline-font-size, 13px); }
  /* Roam 风格:展开且有子节点时,自 bullet 下方引一根竖直缩进导引线贯穿子节点。
     left = 该节点 bullet 中心 = 行左内边距(depth*1.5em+--outline-indent) + 半个 bullet(0.5em);
     子节点 bullet 中心在其右一级(+1.5em),故引导线恰在子节点左侧沟槽。 */
  .node.has-guide::before {
    content: '';
    position: absolute;
    top: var(--outline-line-height, 1.5em);
    bottom: 0.15em;
    left: calc(var(--depth) * 1.5em + (var(--outline-indent, 0.95) + 0.5) * 1em);
    width: 1px;
    background: color-mix(in srgb, currentColor 18%, transparent);
    pointer-events: none;
  }
  .row {
    display: flex; align-items: flex-start; gap: 4px;
    position: relative;
    /* bullet 是行首元素;每级固定缩进 1.5em;左侧留 --outline-indent 沟槽给悬浮的 tri 与引导线 */
    padding: 1px 4px 1px calc(var(--depth) * 1.5em + var(--outline-indent, 0.95) * 1em);
    border-radius: 4px;
    font-family: var(--outline-font-family);
    font-size: var(--outline-font-size, 13px);
    line-height: var(--outline-line-height, 1.5);
  }
  .row.drop-sibling { box-shadow: 0 2px 0 var(--accent-color, #4a80d4); }
  .row.drop-child { box-shadow: inset 2px 0 0 var(--accent-color, #4a80d4); background: #4a80d411; }
  /* 选中态：与原生文字选区同源（系统 Highlight 色）,无竖条无圆角 */
  .row.selected {
    background: Highlight;
    border-radius: 0;
  }
  .row.auto .content { opacity: 0.92; }
  .tri {
    background: none; border: none; padding: 0;
    /* Buttons inherit NEITHER font-family NOR font-size — force BOTH to the outline
       font so the ▾ renders in the theme font AND so the em used by left/width/height
       equals the row's (else tri drifts whenever the theme size ≠ the UA button size,
       e.g. in Daily Notes). ▾ visual size is then reined in via scale(). */
    font-family: inherit;
    font-size: var(--outline-font-size, 13px);
    /* 绝对悬浮在 bullet 左侧沟槽,不占行宽 → 不挤压 bullet、不破坏逐级缩进。
       tri 恒在 bullet 左侧 0.95em(= --outline-indent 沟槽内);根节点(depth 0)
       此时 left=0,tri 正好贴齐大纲左缘,消除顶层项前的多余留白。 */
    position: absolute;
    left: calc(var(--depth) * 1.5em + (var(--outline-indent, 0.95) - 0.95) * 1em);
    top: 1px; /* = 行 padding-top,使盒顶与首行行盒顶对齐 */
    display: inline-flex; align-items: center; justify-content: center;
    width: 1em;
    /* 盒高 = 第一行行高,▾ 居中即落在首行垂直中点 */
    height: var(--outline-line-height, 1.5em);
    line-height: 1;
    cursor: pointer; opacity: 0;
    transform: scale(0.7);
    transition: transform 0.1s, opacity 0.1s;
  }
  /* Roam 风格：折叠三角默认隐藏,鼠标浮到行上才显示 */
  .row:hover .tri { opacity: 0.6; }
  .tri.closed { transform: rotate(-90deg) scale(0.7); }
  .bullet {
    position: relative;
    display: inline-block;
    width: 1em;
    /* 盒高 = 第一行行高(见 .tri 注释,变量是像素值) */
    height: var(--outline-line-height, 1.5em);
    cursor: default; opacity: 0.7;
  }
  /* 圆点：CSS 绘制,绝对居中于 bullet 盒 → 落在首行垂直中点 */
  .bullet::after {
    content: '';
    position: absolute;
    left: 50%; top: 50%;
    width: 0.32em; height: 0.32em;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: currentColor;
  }
  .bullet.jumpable, .bullet.has-kids { cursor: pointer; }
  /* Roam 风格：折叠(有隐藏子节点)时,bullet 外套灰色同心圆环 */
  .bullet.closed::before {
    content: '';
    position: absolute;
    left: 50%; top: 50%;
    width: 0.75em; height: 0.75em;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: color-mix(in srgb, currentColor 20%, transparent);
    z-index: -1;
  }
  .bullet.src-toc { color: var(--accent-color, #4a80d4); }
  .bullet.src-hl { color: #d4a94a; }
  .bullet.src-wl { color: #3aa99f; }
  .bullet.src-anno { color: #b8860b; }
  .bullet.src-note { color: color-mix(in srgb, #b8860b 55%, transparent); }
  .content {
    flex: 1; min-width: 0; white-space: pre-wrap; word-break: break-word; cursor: text;
    /* 面板整体 user-select:none；节点文字本身保留可选（跨行拖动由多选接管） */
    user-select: text; -webkit-user-select: text;
    /* 空内容时高度塌陷为 0，点击/文本光标落不到 span 上 → 保底一行高 */
    min-height: calc(1em * var(--outline-line-height, 1.5));
  }
  /* toc 灰色跟随主题前景色（GrayText 是系统色,不随 theme 变化） */
  .content.src-toc, textarea.src-toc { color: color-mix(in srgb, currentColor 45%, transparent); }
  /* 只读(auto)可跳转节点:点击文字定位到源 md → 指针光标 + hover 下线提示。 */
  .content.jump-src { cursor: pointer; }
  .content.jump-src:hover { text-decoration: underline; text-underline-offset: 2px; }
  .content.hl,
  textarea.hl {
    text-decoration: underline;
    text-decoration-color: var(--highlight-underline, #e0a500);
    text-decoration-thickness: 2px;
    text-underline-offset: 2px;
  }
  textarea.edit {
    resize: none; overflow: hidden; border: none; outline: none;
    border-radius: 3px; background: transparent; color: inherit; font: inherit; padding: 0;
  }
  .qchip {
    align-self: center;
    margin-left: 6px;
    padding: 0 7px;
    border: none; border-radius: 8px; cursor: pointer;
    font-family: inherit;
    font-size: calc(var(--outline-font-size, 13px) * 0.72);
    line-height: 1.7;
    color: #fff;
    background: var(--accent-color, #4a80d4);   /* open */
  }
  .qchip.st-answered { background: #2da44e; }
  .qchip.st-closed {
    color: color-mix(in srgb, currentColor 55%, transparent);
    background: color-mix(in srgb, currentColor 12%, transparent);
  }
</style>
