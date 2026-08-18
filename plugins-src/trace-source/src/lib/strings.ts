// src/lib/strings.ts — self-contained i18n for the trace-source plugin.
//
// A plugin window can't import the host's i18n store, so this mirrors its
// shape in miniature (same pattern as idea-spark / decision-log): a MessageKey
// union, one catalog per locale, and a `t()` that falls back to English.
// Language is chosen from `notemd.locale` at startup via `setLocale`.
//
// `MESSAGE_KEYS` is the single source of truth for the key list — `MessageKey`
// is derived from it so the union type and the runtime-iterable array can
// never drift apart.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export const MESSAGE_KEYS = [
  'delegate',
  'delegating',
  'delegateEmpty',
  'delegated',
  'traceDone',
  'traceFailed',
  'notifyOk',
  'notifyFail',
  'newTrace',
  'placeholder',
  'needVault',
  'agentMissing',
  'agentMissingHint',
  'editorUnavailable',
  'close',
  'cancel',
  'save',
  // The report inbox (right-hand panel) and its row menu.
  'inbox',
  'hideInbox',
  'inboxEmpty',
  'listUnavailable',
  'menuOpenReport',
  'menuDelete',
  'confirmDeleteTitle',
  'confirmDeleteBody',
  'confirmDelete',
  // Settings popover: report directory + the editable delegation prompt.
  'settings',
  'traceDir',
  'prompts',
  'promptsHint',
  'promptMain',
  'promptMissing',
  // The agent picker beside the delegate button. Same keys, same wording as
  // every other surface that offers to run something with an agent — the
  // control is a standard, so its vocabulary is too.
  'agentPicker.by',
  'agentPicker.model',
  'agentPicker.unknown',
  'agentPicker.notInstalled',
  'agentPicker.broken',
] as const

export type MessageKey = (typeof MESSAGE_KEYS)[number]

type Catalog = Record<MessageKey, string>

// English is the baseline catalog: every other locale is checked against it
// (see strings.test.ts) for key coverage.
const en: Catalog = {
  delegate: 'Trace it',
  delegating: 'Delegating…',
  delegateEmpty: 'Paste or write the passage to trace first.',
  delegated: 'Delegated — a notification will arrive when the report is ready.',
  traceDone: 'Trace finished — the report is in the inbox.',
  traceFailed: 'Tracing failed',
  notifyOk: 'Trace report ready',
  notifyFail: 'Tracing failed',
  newTrace: 'New trace',
  placeholder:
    'Paste the passage to trace back to its source. Add scope notes if you like — e.g. "only YouTube and arxiv".',
  needVault: 'Open a vault first.',
  agentMissing: 'No agent plugin found',
  agentMissingHint: 'Install an agent plugin (e.g. Claude Agent) from the marketplace to enable tracing.',
  editorUnavailable: 'The rich editor could not be loaded — falling back to plain text.',
  close: 'Close',
  cancel: 'Cancel',
  save: 'Save',
  inbox: 'Inbox',
  hideInbox: 'Hide inbox',
  inboxEmpty: 'No trace reports yet.',
  listUnavailable: 'The report folder could not be read — the inbox may be incomplete.',
  menuOpenReport: 'Open in the main editor',
  menuDelete: 'Delete',
  confirmDeleteTitle: 'Delete this report?',
  confirmDeleteBody: 'This deletes the following files for good — there is no trash:',
  confirmDelete: 'Delete',
  settings: 'Settings',
  traceDir: 'Report folder',
  prompts: 'Agent prompts',
  promptsHint: 'Opens in the main editor. Your edits stay — they are never overwritten.',
  promptMain: 'Trace a passage',
  promptMissing: 'This task has no CLAUDE.md prompt to edit.',
  'agentPicker.by': 'by {name}',
  'agentPicker.model': 'model {model}',
  'agentPicker.unknown': 'harness unknown',
  'agentPicker.notInstalled': 'not installed',
  'agentPicker.broken': 'found, but it will not start',
}

const zh: Catalog = {
  delegate: '开始溯源',
  delegating: '委托中…',
  delegateEmpty: '先粘贴或写下要溯源的那段话。',
  delegated: '已委托 —— 报告写好后会收到通知。',
  traceDone: '溯源完成 —— 报告已进收件箱。',
  traceFailed: '溯源失败',
  notifyOk: '溯源完成',
  notifyFail: '溯源失败',
  newTrace: '新溯源',
  placeholder: '粘贴要溯源的文字;可补充范围说明,如「只查 YouTube 和 arxiv」。',
  needVault: '请先打开一个 vault。',
  agentMissing: '未找到 agent 插件',
  agentMissingHint: '请从插件市场安装一个 agent 插件(如 Claude 智能体)以启用溯源。',
  editorUnavailable: '富文本编辑器加载失败 —— 已退回纯文本编辑。',
  close: '关闭',
  cancel: '取消',
  save: '保存',
  inbox: '收件箱',
  hideInbox: '隐藏收件箱',
  inboxEmpty: '还没有溯源报告。',
  listUnavailable: '读取报告目录失败 —— 收件箱可能不完整。',
  menuOpenReport: '在主编辑器打开',
  menuDelete: '删除',
  confirmDeleteTitle: '删除这份报告?',
  confirmDeleteBody: '以下文件会被彻底删除,没有回收站:',
  confirmDelete: '删除',
  settings: '设置',
  traceDir: '报告目录',
  prompts: '委托提示词',
  promptsHint: '点开即在主编辑器里编辑;你改过的内容不会被覆盖。',
  promptMain: '溯源',
  promptMissing: '这个任务没有可编辑的 CLAUDE.md 提示词。',
  'agentPicker.by': '由 {name} 执行',
  'agentPicker.model': '模型 {model}',
  'agentPicker.unknown': '运行环境未知',
  'agentPicker.notInstalled': '未安装',
  'agentPicker.broken': '装了,但起不来',
}

const ja: Catalog = {
  delegate: '出典をたどる',
  delegating: '委任中…',
  delegateEmpty: 'まず、たどりたい一節を貼り付けるか書いてください。',
  delegated: '委任しました —— レポートができたら通知が届きます。',
  traceDone: '追跡が完了しました —— レポートはインボックスにあります。',
  traceFailed: '出典の追跡に失敗しました',
  notifyOk: '出典レポートが完成しました',
  notifyFail: '出典の追跡に失敗しました',
  newTrace: '新しい追跡',
  placeholder: 'たどりたい文章を貼り付けてください。「YouTube と arxiv だけ」のような範囲の指定も書けます。',
  needVault: '先に vault を開いてください。',
  agentMissing: 'エージェント プラグインが見つかりません',
  agentMissingHint: '追跡を有効にするには、マーケットプレイスからエージェント プラグイン(例:Claude エージェント)をインストールしてください。',
  editorUnavailable: 'リッチエディタを読み込めませんでした —— プレーンテキストに切り替えます。',
  close: '閉じる',
  cancel: 'キャンセル',
  save: '保存',
  inbox: 'インボックス',
  hideInbox: 'インボックスを隠す',
  inboxEmpty: 'まだ出典レポートがありません。',
  listUnavailable: 'レポートフォルダを読み取れませんでした —— インボックスが不完全な可能性があります。',
  menuOpenReport: 'メインエディタで開く',
  menuDelete: '削除',
  confirmDeleteTitle: 'このレポートを削除しますか?',
  confirmDeleteBody: '次のファイルを完全に削除します(ごみ箱はありません):',
  confirmDelete: '削除',
  settings: '設定',
  traceDir: 'レポートフォルダ',
  prompts: '委任プロンプト',
  promptsHint: 'クリックするとメインエディタで開きます。編集した内容が上書きされることはありません。',
  promptMain: '出典をたどる',
  promptMissing: 'このタスクには編集できる CLAUDE.md プロンプトがありません。',
  'agentPicker.by': '実行:{name}',
  'agentPicker.model': 'モデル {model}',
  'agentPicker.unknown': '実行環境不明',
  'agentPicker.notInstalled': '未インストール',
  'agentPicker.broken': 'インストール済みですが起動できません',
}

const de: Catalog = {
  delegate: 'Quelle finden',
  delegating: 'Delegiere…',
  delegateEmpty: 'Füge zuerst die Passage ein, deren Quelle gesucht werden soll.',
  delegated: 'Delegiert — eine Benachrichtigung kommt, sobald der Bericht fertig ist.',
  traceDone: 'Suche abgeschlossen — der Bericht liegt im Eingang.',
  traceFailed: 'Quellensuche fehlgeschlagen',
  notifyOk: 'Quellenbericht fertig',
  notifyFail: 'Quellensuche fehlgeschlagen',
  newTrace: 'Neue Suche',
  placeholder: 'Füge die Passage ein, deren Ursprung gesucht werden soll. Eingrenzungen sind willkommen — z. B. „nur YouTube und arxiv“.',
  needVault: 'Öffne zuerst einen Vault.',
  agentMissing: 'Kein Agent-Plugin gefunden',
  agentMissingHint: 'Installiere ein Agent-Plugin (z. B. Claude Agent) aus dem Marktplatz, um die Quellensuche zu aktivieren.',
  editorUnavailable: 'Der Rich-Text-Editor konnte nicht geladen werden — Rückfall auf reinen Text.',
  close: 'Schließen',
  cancel: 'Abbrechen',
  save: 'Speichern',
  inbox: 'Eingang',
  hideInbox: 'Eingang ausblenden',
  inboxEmpty: 'Noch keine Quellenberichte.',
  listUnavailable: 'Der Berichtsordner konnte nicht gelesen werden — der Eingang ist womöglich unvollständig.',
  menuOpenReport: 'Im Haupteditor öffnen',
  menuDelete: 'Löschen',
  confirmDeleteTitle: 'Diesen Bericht löschen?',
  confirmDeleteBody: 'Die folgenden Dateien werden endgültig gelöscht — es gibt keinen Papierkorb:',
  confirmDelete: 'Löschen',
  settings: 'Einstellungen',
  traceDir: 'Berichtsordner',
  prompts: 'Agent-Prompts',
  promptsHint: 'Öffnet sich im Haupteditor. Deine Änderungen bleiben — sie werden nie überschrieben.',
  promptMain: 'Quelle finden',
  promptMissing: 'Diese Aufgabe hat keinen CLAUDE.md-Prompt zum Bearbeiten.',
  'agentPicker.by': 'via {name}',
  'agentPicker.model': 'Modell {model}',
  'agentPicker.unknown': 'Umgebung unbekannt',
  'agentPicker.notInstalled': 'nicht installiert',
  'agentPicker.broken': 'vorhanden, startet aber nicht',
}

const registry: Record<Locale, Catalog> = { en, zh, ja, de }

let active: Locale = 'en'

function isLocale(v: unknown): v is Locale {
  return v === 'en' || v === 'zh' || v === 'ja' || v === 'de'
}

/**
 * Sets the active locale from `notemd.locale`. Accepts a region suffix
 * (`zh-CN` → `zh`); unknown/absent falls back to English.
 */
export function setLocale(code: string | undefined): void {
  const base = code?.split('-')[0]
  active = isLocale(base) ? base : 'en'
}

/** Translates `key` for the active locale, falling back to English then the raw key. */
export function t(key: MessageKey): string {
  const catalog = registry[active] ?? en
  return catalog[key] ?? en[key] ?? key
}

/**
 * `t` with `{placeholder}` substitution — the shared agent picker takes a
 * `(key, vars) => string`. An unknown placeholder is left as written rather
 * than blanked: a visible `{model}` is a bug report, an empty space a mystery.
 */
export function tv(key: MessageKey, vars?: Record<string, string | number>): string {
  const s = t(key)
  if (!vars) return s
  return s.replace(/\{(\w+)\}/g, (whole, name) =>
    name in vars ? String(vars[name]) : whole,
  )
}

/**
 * The locale `setLocale` settled on. Exported for the Intl formatters the UI
 * builds itself (relative times in the inbox) — those need a locale code, and
 * must not guess one from `navigator.language` when the host has already
 * resolved what language this window is speaking.
 */
export function locale(): Locale {
  return active
}

// Exported for tests only (catalog completeness checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
