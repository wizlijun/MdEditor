<script lang="ts">
  import type { RecallTreeNode } from '../../lib/outline/recall'
  import InlineRender from './InlineRender.svelte'
  import RefTreeNode from './RefTreeNode.svelte'
  import { isImeKey } from '../../lib/ime'

  // Collapsible renderer for one recalled subtree node. When `editable`, the
  // text can be edited in place (Phase B / B1): commit writes back to source.
  let {
    node,
    defaultCollapsed = false,
    editable = false,
    onCommit,
    onPageClick,
  }: {
    node: RecallTreeNode
    defaultCollapsed?: boolean
    editable?: boolean
    onCommit?: (path: number[], oldText: string, newText: string) => void | Promise<unknown>
    onPageClick?: (target: string) => void
  } = $props()

  let collapsed = $state(defaultCollapsed)
  const hasChildren = $derived(node.children.length > 0)

  let editing = $state(false)
  let draft = $state('')

  function startEdit(e?: MouseEvent) {
    if (!editable) return
    // Let a wikilink / hashtag / link handle its own click — don't hijack it
    // into edit mode (they navigate via onPageClick or href).
    if ((e?.target as HTMLElement | undefined)?.closest('.pl, a')) return
    draft = node.text
    editing = true
  }
  function commit() {
    if (!editing) return
    editing = false
    if (draft !== node.text) void onCommit?.(node.path, node.text, draft)
  }
  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation()
    // 变换中的回车是在确认候选，不是在提交这一行（见 src/lib/ime.ts）
    if (isImeKey(e)) return
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); commit() }
    else if (e.key === 'Escape') { e.preventDefault(); editing = false }
  }
  function autofocusGrow(el: HTMLTextAreaElement) {
    const grow = () => { el.style.height = 'auto'; el.style.height = el.scrollHeight + 'px' }
    el.focus(); el.setSelectionRange(el.value.length, el.value.length); grow()
    el.addEventListener('input', grow)
    return { destroy() { el.removeEventListener('input', grow) } }
  }
</script>

<div class="ref-node" class:has-guide={hasChildren && !collapsed}>
  <div class="ref-row">
    {#if hasChildren}
      <!-- Fold caret: hidden in the gutter until the row is hovered (Roam style),
           mirrors OutlineNode's .tri. -->
      <button class="twist" class:closed={collapsed} onclick={() => (collapsed = !collapsed)} aria-label="toggle">▾</button>
    {/if}
    <!-- CSS-drawn bullet (mirrors OutlineNode's .bullet); a collapsed parent gets
         the grey ring. Clicking a parent bullet toggles its fold. -->
    <span
      class="bullet" class:has-kids={hasChildren} class:closed={hasChildren && collapsed}
      onclick={() => { if (hasChildren) collapsed = !collapsed }}
      role="presentation"
    ></span>
    {#if editing}
      <textarea class="edit" rows="1" bind:value={draft} onkeydown={onKeydown} onblur={commit} use:autofocusGrow></textarea>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions a11y_no_noninteractive_tabindex -->
      <span
        class="text" class:editable
        role={editable ? 'button' : undefined}
        tabindex={editable ? 0 : undefined}
        onclick={startEdit}
        onkeydown={(e) => { if (editable && e.key === 'Enter') { e.preventDefault(); startEdit() } }}
      ><InlineRender content={node.text} {onPageClick} /></span>
    {/if}
  </div>
  {#if hasChildren && !collapsed}
    <div class="children">
      {#each node.children as c, i (i)}
        <!-- Nested ref children also start collapsed so Linked References stays compact. -->
        <RefTreeNode node={c} {editable} {onCommit} {onPageClick} defaultCollapsed={true} />
      {/each}
    </div>
  {/if}
</div>

<style>
  /* Inherit the main outline's typography + Roam-style bullet/fold/guide so the
     references read as one outline with the SAME base style as OutlineNode.
     font-size on .ref-node so its em (children indent, guide offset) scale with
     the theme size, exactly like OutlineNode's .node. */
  .ref-node { position: relative; font-size: var(--outline-font-size, 13px); }
  /* Vertical guide line under an expanded parent (mirrors .node.has-guide::before).
     left = bullet center = row gutter (1.7em) + half bullet (0.5em) = 2.2em. */
  .ref-node.has-guide::before {
    content: '';
    position: absolute;
    top: var(--outline-line-height, 1.5em);
    bottom: 0.15em;
    left: 2.2em;
    width: 1px;
    background: color-mix(in srgb, currentColor 18%, transparent);
    pointer-events: none;
  }
  .ref-row {
    display: flex; align-items: flex-start; gap: 4px;
    position: relative;
    /* 1.7em left gutter holds the hover-revealed caret; bullet sits at the edge. */
    padding: 1px 0 1px 1.7em;
    font-family: var(--outline-font-family);
    font-size: var(--outline-font-size, 13px);
    line-height: var(--outline-line-height, 1.5);
  }
  /* Fold caret — absolutely floated in the gutter, hidden until row hover, ▾ reined
     in with scale() so its em box matches the outline size (mirrors .tri). */
  .twist {
    background: none; border: none; padding: 0; color: inherit; font-family: inherit;
    font-size: var(--outline-font-size, 13px);
    position: absolute; left: 0.75em; top: 1px;
    display: inline-flex; align-items: center; justify-content: center;
    width: 1em; height: var(--outline-line-height, 1.5em); line-height: 1;
    cursor: pointer; opacity: 0; transform: scale(0.7);
    transition: transform 0.1s, opacity 0.1s;
  }
  .ref-row:hover .twist { opacity: 0.6; }
  .twist.closed { transform: rotate(-90deg) scale(0.7); }
  /* Bullet — CSS-drawn dot centered in a line-height-tall box (mirrors .bullet). */
  .bullet {
    position: relative; display: inline-block; flex: none;
    width: 1em; height: var(--outline-line-height, 1.5em); opacity: 0.7;
  }
  .bullet.has-kids { cursor: pointer; }
  .bullet::after {
    content: '';
    position: absolute; left: 50%; top: 50%;
    width: 0.32em; height: 0.32em;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: currentColor;
  }
  /* Collapsed parent: grey ring around the dot (mirrors .bullet.closed::before). */
  .bullet.closed::before {
    content: '';
    position: absolute; left: 50%; top: 50%;
    width: 0.75em; height: 0.75em;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: color-mix(in srgb, currentColor 20%, transparent);
    z-index: -1;
  }
  .text { flex: 1; min-width: 0; }
  .text.editable { cursor: text; border-radius: 3px; }
  .text.editable:hover { background: var(--hover-bg, #8881); }
  /* Match the main outline's edit textarea (textarea.edit + .content). */
  .edit {
    flex: 1; min-width: 0; resize: none; overflow: hidden;
    border: none; outline: none; border-radius: 3px;
    background: transparent; color: inherit; font: inherit; padding: 0; margin: 0;
    white-space: pre-wrap; word-break: break-word;
    line-height: var(--outline-line-height, 1.5);
  }
  /* Per-level indent matches OutlineNode's 1.5em step so nesting lines up. */
  .children { padding-left: 1.5em; }
</style>
