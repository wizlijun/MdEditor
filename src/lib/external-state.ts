/**
 * Pure decision function for external file-change detection.
 *
 * Given the tab's current state and a fresh disk event, returns what the
 * caller should do. Side-effect-free; trivially testable. Lives outside
 * the Svelte runes so we can unit-test without a DOM.
 */

export interface DiskSnapshot {
  /** Last-modification time, ms since epoch. */
  mtime: number
  /** sha256 hex of the disk content. */
  hash: string
  /** The freshly-read content (UTF-8). */
  content: string
}

export type ExternalEvent =
  | { type: 'modified'; snapshot: DiskSnapshot }
  | { type: 'deleted' }

export interface TabSnapshot {
  initialContent: string
  currentContent: string
  lastKnownMtime: number
  lastKnownHash: string
  externalState: 'fresh' | 'changed' | 'deleted'
  /**
   * Editor mode. Rich mode owns its own internal document state, which
   * cannot be silently resynced without surprising the user — so rich-mode
   * tabs always surface the banner instead of taking the autoReload
   * fast-path. Source mode is a plain controlled textarea and reloads
   * cleanly.
   */
  mode: 'source' | 'rich'
  /**
   * 该 tab 是大纲笔记(`.note.md`)。它由 OutlineEditor 渲染,用的是可随时从文本
   * 重建的 outline store —— 没有 ProseMirror 那种「内部状态会把旧内容反向写回磁盘」
   * 的顾虑,所以内容干净时允许走 autoReload 快路径(mode 名义上仍是 rich)。
   */
  isOutlineNote?: boolean
}

export type Decision =
  | { kind: 'ignore' }
  | { kind: 'autoReload'; snapshot: DiskSnapshot }
  | { kind: 'showChanged'; snapshot: DiskSnapshot }
  | { kind: 'showDeleted' }

export function decide(tab: TabSnapshot, event: ExternalEvent): Decision {
  if (event.type === 'deleted') {
    return tab.externalState === 'deleted' ? { kind: 'ignore' } : { kind: 'showDeleted' }
  }
  // event.type === 'modified'
  const { hash } = event.snapshot
  // Hash equality usually means "disk content matches what we last accepted"
  // → ignore (covers self-write echoes and external touch with no content
  // change). Exception: when the tab is currently `deleted`, a modify event
  // means the file was recreated and we MUST transition state, even if the
  // recreated content happens to equal the pre-deletion content.
  if (hash === tab.lastKnownHash && tab.externalState !== 'deleted') {
    return { kind: 'ignore' }
  }
  const dirty = tab.currentContent !== tab.initialContent
  // Rich-mode tabs never autoReload: the editor's internal state would still
  // show the pre-change content, and the next keystroke or destroy-flush
  // would silently overwrite the disk's new version. Force the banner so
  // the user explicitly chooses Reload vs. Overwrite.
  if (dirty || (tab.mode === 'rich' && !tab.isOutlineNote)) {
    return { kind: 'showChanged', snapshot: event.snapshot }
  }
  return { kind: 'autoReload', snapshot: event.snapshot }
}
