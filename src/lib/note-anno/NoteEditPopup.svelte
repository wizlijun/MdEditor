<script lang="ts">
  import { noteUi, styleVars } from './note-ui.svelte'
  import { t } from '../i18n/store.svelte'
  import { iconSvg } from '../context-menu/icons'

  // Captured at mount: the parent only mounts this while noteUi.edit is set.
  const editState = noteUi.edit!
  let text = $state(editState.note)
  let root: HTMLDivElement | undefined = $state()
  let ta: HTMLTextAreaElement | undefined = $state()

  // 默认全选(编辑已有批注:直接改写);带 caret 时收起选区停在该位置
  // (提问:预填的 ? 不能被首个按键吃掉)
  $effect(() => {
    ta?.focus()
    if (editState.caret == null) ta?.select()
    else ta?.setSelectionRange(editState.caret, editState.caret)
  })

  // 输入即保存（防抖）：大纲/徽标从文档派生，等到关闭弹窗才写入会显得"不更新"。
  // save() 自带 no-op 判断，重复提交同文本无副作用。
  let saveTimer: ReturnType<typeof setTimeout> | null = null
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = setTimeout(() => {
      saveTimer = null
      if (noteUi.edit === editState) editState.save(text)
    }, 300)
  }

  function close(save: boolean) {
    if (noteUi.edit !== editState) return
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }
    noteUi.edit = null
    if (save) editState.save(text)
  }
  function onDelete() {
    if (noteUi.edit !== editState) return
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }
    noteUi.edit = null
    editState.remove()
  }
  /** ⁇ 提问:尾部无问号则补全角 ？,保存并关闭。问题身份由文本中的问号承载,按钮只是糖 */
  function onAsk() {
    if (!/[?？]\s*$/.test(text)) text = text.trimEnd() + '？'
    close(true)
  }
  function onWindowMousedown(e: MouseEvent) {
    if (root && !root.contains(e.target as Node)) close(true)
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.stopPropagation(); close(true) }
    // Notes are single-line (newlines are flattened on save) → Enter confirms.
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); e.stopPropagation(); close(true) }
  }
</script>

<svelte:window onmousedown={onWindowMousedown} />

<div
  class="note-edit menu-panel"
  bind:this={root}
  style="left:{editState.x}px; top:{editState.y}px; {styleVars(editState.style)}"
  onkeydown={onKeydown}
  role="dialog"
  aria-label={t('ctxmenu.note')}
  tabindex="-1"
>
  <textarea
    bind:this={ta}
    bind:value={text}
    oninput={scheduleSave}
    rows="3"
    placeholder={t('noteedit.placeholder')}
  ></textarea>
  <div class="row">
    <button class="ask" onclick={onAsk} title={t('noteedit.askHint')}>⁇ {t('noteedit.ask')}</button>
    <button
      class="del"
      onclick={onDelete}
      title={t('noteedit.delete')}
      aria-label={t('noteedit.delete')}
    >{@html iconSvg('trash')}</button>
  </div>
</div>

<style>
  /* Chrome comes from the shared .menu-panel class in app.css.
     --note-bg/--note-fg carry the current document theme's rendered surface
     and text colors (set inline when the popup opens); they fall back to the
     system Canvas/CanvasText chrome when no theme colors were captured. */
  .note-edit {
    position: fixed;
    z-index: 1001;
    width: 280px;
    padding: 6px;
    font: inherit;
    font-size: 13px;
    background: color-mix(in srgb, var(--note-bg, Canvas) 82%, transparent);
    color: var(--note-fg, CanvasText);
    border-color: color-mix(in srgb, var(--note-fg, CanvasText) 22%, transparent);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: none;
    /* Typography follows the document theme (serif body type, line spacing, …)
       so what you type matches how the annotation renders in the page. */
    font-family: var(--note-font-family, inherit);
    font-size: var(--note-font-size, 13px);
    font-weight: var(--note-font-weight, 400);
    font-style: var(--note-font-style, normal);
    line-height: var(--note-line-height, 1.5);
    letter-spacing: var(--note-letter-spacing, normal);
    font-feature-settings: var(--note-font-feature, normal);
    color: var(--note-fg, CanvasText);
    background: color-mix(in srgb, var(--note-fg, CanvasText) 5%, var(--note-bg, Canvas));
    border: none;
    border-radius: 5px;
    padding: 6px 8px;
    outline: none;
  }
  textarea:focus {
    background: color-mix(in srgb, var(--note-fg, CanvasText) 9%, var(--note-bg, Canvas));
  }
  textarea::placeholder {
    color: color-mix(in srgb, var(--note-fg, CanvasText) 45%, transparent);
  }
  .row { display: flex; justify-content: space-between; margin-top: 4px; }
  .ask {
    display: flex; align-items: center; gap: 4px;
    padding: 3px 10px;
    border: none; border-radius: 5px; cursor: pointer;
    font-family: inherit; font-size: 12px; font-weight: 600;
    color: #fff;
    background: var(--accent-color, #4a80d4);
  }
  .ask:hover { filter: brightness(1.1); }
  .del {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 5px;
    cursor: pointer;
    color: color-mix(in srgb, var(--note-fg, CanvasText) 65%, var(--note-bg, Canvas));
  }
  .del:hover {
    background: color-mix(in srgb, var(--note-fg, CanvasText) 10%, var(--note-bg, Canvas));
    color: var(--note-fg, CanvasText);
  }
  /* iconSvg() emits class="ctx-icon" inside {@html} — style it unscoped. */
  :global(.note-edit .ctx-icon) { width: 15px; height: 15px; display: block; }
</style>
