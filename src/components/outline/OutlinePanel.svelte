<script lang="ts">
  import type { Tab } from '../../lib/tabs.svelte'
  import { outlineAppliesTo } from '../../lib/outline/gate.svelte'
  import { setSideVisible } from '../../lib/side-panel/registry.svelte'
  import { t } from '../../lib/i18n/store.svelte'
  import { companionPathFor } from '../../lib/outline/store.svelte'
  import { noteHomeForRead } from '../../lib/outline/note-home'
  import { sotvaultStore } from '../../lib/sotvault.svelte'
  import { openFile, setMode, closeTab, tabs } from '../../lib/tabs.svelte'
  import OutlineEditor from './OutlineEditor.svelte'
  import SideViewSwitcher from '../side-panel/SideViewSwitcher.svelte'
  import AgentWorkspace from './AgentWorkspace.svelte'
  import { isAgentBusy } from '../../lib/agent-workspace/store.svelte'

  let { tab }: { tab: Tab | null } = $props()

  type OutlineEditorApi = { prepareForAgent: () => Promise<string | null> }
  let outlineEditor = $state<OutlineEditorApi | null>(null)

  // Whether the current tab has an outline. Drives body state + button enablement.
  let applicable = $derived(tab != null && outlineAppliesTo(tab))
  // Vault-homed resolution, same as OutlineEditor's: a synced source's note lives
  // next to the VAULT copy, not next to the source. Using the bare source-side
  // companion here handed the agent (and 打开/删除笔记) a path that doesn't exist.
  let companionPath = $derived(
    applicable && tab
      ? (noteHomeForRead(tab.filePath, {
          vaultRoot: sotvaultStore.vaultRoot,
          records: sotvaultStore.records,
        }) ?? companionPathFor(tab.filePath))
      : null,
  )

  // 面板重置计数：删除笔记后自增 → OutlineEditor 重挂 → 重读(文件已无) → 空大纲
  let resetTick = $state(0)

  // 铅笔菜单（fixed 定位，右边缘对齐按钮右边缘 → 向左展开，避免右侧被裁）
  let menu = $state<{ open: boolean; right: number; y: number }>({ open: false, right: 0, y: 0 })
  let noteExists = $state(false)
  async function toggleMenu(e: MouseEvent) {
    if (menu.open) { menu = { open: false, right: 0, y: 0 }; return }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
    noteExists = companionPath
      ? await (await import('@tauri-apps/plugin-fs')).exists(companionPath).catch(() => false)
      : false
    menu = { open: true, right: window.innerWidth - r.right, y: r.bottom + 2 }
  }
  function closeMenu() { menu = { open: false, right: 0, y: 0 } }

  async function openMarkdown() {
    closeMenu()
    if (!companionPath) return
    const { exists } = await import('@tauri-apps/plugin-fs')
    if (await exists(companionPath).catch(() => false)) {
      await openFile(companionPath)
    } else {
      // 惰性：磁盘上还没笔记 → 打开未保存 buffer（同样以源码显示模板）
      const [{ openNewOutlineTab }, { pageNameOf }, { newOutlineFileText }] = await Promise.all([
        import('../../lib/tabs.svelte'),
        import('../../lib/outline/backlinks'),
        import('../../lib/outline/create'),
      ])
      await openNewOutlineTab(companionPath, newOutlineFileText(pageNameOf(companionPath)))
    }
    const opened = tabs.find((x) => x.filePath === companionPath)
    if (opened) setMode(opened.id, 'source')   // source 路由先于 isOutlineNoteTab → 原始 Markdown
  }

  async function deleteNote() {
    closeMenu()
    if (!companionPath) return
    const { exists, remove } = await import('@tauri-apps/plugin-fs')
    if (!(await exists(companionPath).catch(() => false))) return
    const { ask } = await import('@tauri-apps/plugin-dialog')
    const ok = await ask(t('outline.deleteNoteConfirm'), {
      title: t('outline.deleteNote'), kind: 'warning',
      okLabel: t('outline.deleteNote'), cancelLabel: t('common.cancel'),
    })
    if (!ok) return
    const openTab = tabs.find((x) => x.filePath === companionPath)
    if (openTab) await closeTab(openTab.id, async () => 'discard' as const)   // 删前不保存
    await remove(companionPath).catch((e) => console.warn('[outline] delete note failed:', e))
    resetTick++
  }

  // While the agent rewrites this note, the outline must not be edited — its
  // in-memory tree would be written back over the agent's work.
  const agentBusy = $derived(isAgentBusy())

  async function prepareNoteForAgent(): Promise<string | null> {
    return outlineEditor?.prepareForAgent() ?? null
  }

  /**
   * After a run: re-read the note from disk (remounting OutlineEditor re-reads
   * it) and, when the tab has no unsaved edits of its own, the source document
   * too. A dirty tab is left alone — the user's typing outranks a refresh.
   */
  async function refreshAfterAgent() {
    resetTick++
    const path = tab?.filePath
    if (!path) return
    // The ✦ cards in the main editor read the sidecar note. `loadAnswersFor`
    // would serialize the outline's IN-MEMORY tree while it holds this note —
    // the very copy the agent just superseded — so feed it the text from disk.
    if (companionPath) {
      const fs = await import('@tauri-apps/plugin-fs')
      const text = await fs.readTextFile(companionPath).catch(() => null)
      if (text != null) {
        const { setAnswersFromText } = await import('../../lib/note-anno/answers-store.svelte')
        setAnswersFromText(companionPath, text)
      }
    }
    const dirty = tab != null && tab.currentContent !== tab.initialContent
    if (dirty) return
    const { reloadTabFromDisk } = await import('../../lib/tabs.svelte')
    await reloadTabFromDisk(path).catch((e) =>
      console.warn('[agent] reloading the source document failed:', e),
    )
  }

  function onWindowMouseDown(e: MouseEvent) {
    if (!menu.open) return
    const target = e.target as HTMLElement | null
    if (target?.closest('.pencil-menu') || target?.closest('.pencil-btn')) return
    closeMenu()
  }
  function onWindowKeyDown(e: KeyboardEvent) {
    if (menu.open && e.key === 'Escape') { e.preventDefault(); closeMenu() }
  }
</script>

<svelte:window onmousedown={onWindowMouseDown} onkeydown={onWindowKeyDown} />

<div class="outline-content">
  <header>
    <button class="hbtn" title={t('outline.hide')} aria-label={t('outline.hide')} onclick={() => void setSideVisible('right', false)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2" />
        <line x1="15" y1="3" x2="15" y2="21" />
        <polyline points="8 9 11 12 8 15" />
      </svg>
    </button>
    <SideViewSwitcher side="right" {tab} />
    <button class="hbtn pencil-btn" class:on={menu.open} title={t('outline.editNote')} aria-label={t('outline.editNote')} disabled={!companionPath} onclick={toggleMenu}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
      </svg>
    </button>
  </header>
  {#if !applicable || !companionPath}
    <div class="body">
      <p class="empty">{tab == null ? t('outline.noDocument') : t('outline.notApplicable')}</p>
    </div>
  {:else}
    <!-- 全功能大纲编辑器：与主编辑器双向同步（keyed 换文档/删除后整体重挂）。
         agent 工作期间整块 inert：焦点、点击、键盘一律进不来，不必逐个审查
         编辑器内部的交互入口。 -->
    <div class="editor-host" inert={agentBusy}>
      {#key `${tab!.id}:${resetTick}`}
        <OutlineEditor bind:this={outlineEditor} mainTab={tab} />
      {/key}
    </div>
  {/if}
  <AgentWorkspace
    sourcePath={tab?.filePath ?? null}
    notePath={companionPath}
    prepareNote={prepareNoteForAgent}
    onfinished={refreshAfterAgent}
  />
</div>

{#if menu.open}
  <div class="pencil-menu menu-panel" role="menu" style="right: {menu.right}px; top: {menu.y}px">
    <button type="button" role="menuitem" class="pmenu-row menu-row" onclick={() => void openMarkdown()}>{t('outline.openMarkdown')}</button>
    <button type="button" role="menuitem" class="pmenu-row menu-row danger" disabled={!noteExists} onclick={() => void deleteNote()}>{t('outline.deleteNote')}</button>
  </div>
{/if}

<style>
  .outline-content {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    padding: 8px 12px;
    font-size: 13px;
    font-weight: 600;
    border-bottom: 1px solid var(--border-color, #3333);
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .hbtn {
    display: inline-flex; align-items: center; justify-content: center;
    border: 0; background: transparent; cursor: pointer;
    padding: 3px; border-radius: 4px; opacity: 0.7;
  }
  .hbtn svg { display: block; }
  .hbtn:hover:not(:disabled) { background: rgba(0,0,0,0.08); opacity: 1; }
  .hbtn:disabled { opacity: 0.25; cursor: default; }
  .body { flex: 1; overflow-y: auto; padding: 8px; }
  /* Holds the editor's flex share so the agent workspace sits under it. */
  .editor-host { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .editor-host[inert] { opacity: 0.55; }
  .empty { opacity: 0.5; font-size: 12px; }
  /* Chrome + accent hover 来自全局 .menu-panel / .menu-row(见 app.css);
     这里只保留定位、button 复位、以及 delete 行的红色 hover 覆盖。 */
  .pencil-menu { position: fixed; z-index: 9998; min-width: 168px; }
  .pmenu-row {
    width: 100%; text-align: left; border: 0; background: none;
    color: inherit; font: inherit; cursor: pointer;
  }
  .pmenu-row.danger { color: #d24b4b; }
  .pmenu-row.danger:hover:not(:disabled) { background: #d44a4a; color: #fff; }
  .pmenu-row:disabled { opacity: 0.35; cursor: default; pointer-events: none; }
  /* 面板窄容器里收紧编辑器的内边距/宽度约束 */
  .outline-content :global(.outline-editor .body) {
    padding: 10px 12px;
    max-width: none;
  }
  @media (prefers-color-scheme: dark) {
    .hbtn:hover:not(:disabled) { background: rgba(255,255,255,0.1); }
  }
</style>
