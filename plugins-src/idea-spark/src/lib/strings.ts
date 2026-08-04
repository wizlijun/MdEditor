// src/lib/strings.ts — self-contained i18n for the idea-spark plugin.
//
// A plugin window can't import the host's i18n store, so this mirrors its
// shape (src/lib/i18n/store.svelte.ts) in miniature, matching the pattern set
// by plugins-src/decision-log/src/lib/strings.ts: a MessageKey union, one
// catalog per locale, and a `t()` that falls back to English. Language is
// chosen from `notemd.locale` at startup via `setLocale`; see App.svelte.
//
// `MESSAGE_KEYS` is the single source of truth for the key list — `MessageKey`
// is derived from it (`typeof MESSAGE_KEYS[number]`) so the union type and the
// runtime-iterable array can never drift apart.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export const MESSAGE_KEYS = [
  'title',
  'editorPlaceholder',
  'save',
  'saved',
  'delegate',
  'delegateDeferred',
  'delegating',
  'waitHint',
  'settings',
  'ideaDir',
  'history',
  'statusDraft',
  'statusRunning',
  'statusDone',
  'statusFailed',
  'openResult',
  'retry',
  'needVault',
  'agentMissing',
  'agentMissingHint',
  'celebrate',
  'templateH1',
  'templateHint',
  'sectionDomain',
  'sectionTransfer',
  'sectionResources',
  'sectionOutcome',
  'deleteConfirm',
  'close',
] as const

export type MessageKey = (typeof MESSAGE_KEYS)[number]

type Catalog = Record<MessageKey, string>

// English is the baseline catalog: every other locale is checked against it
// (see strings.test.ts) for key coverage.
const en: Catalog = {
  title: 'Idea Spark',
  editorPlaceholder: "What's the spark?",
  save: 'Save',
  saved: 'Saved',
  delegate: 'Delegate to agent',
  delegateDeferred: 'Delegation isn’t wired up yet — coming in a later update.',
  delegating: 'Delegating…',
  waitHint: 'This can take a minute — feel free to keep writing.',
  settings: 'Settings',
  ideaDir: 'Idea folder',
  history: 'History',
  statusDraft: 'Draft',
  statusRunning: 'Running',
  statusDone: 'Done',
  statusFailed: 'Failed',
  openResult: 'Open result',
  retry: 'Retry',
  needVault: 'Open a vault first.',
  agentMissing: 'Claude Agent plugin not found',
  agentMissingHint: 'Install the Claude Agent plugin from the marketplace to enable delegation.',
  celebrate: 'Nice spark!',
  templateH1: 'New idea',
  templateHint: 'Fill in what you can — the agent argues the rest.',
  sectionDomain: 'Domain',
  sectionTransfer: 'Transfer',
  sectionResources: 'Resources',
  sectionOutcome: 'Outcome',
  deleteConfirm: 'Delete this idea?',
  close: 'Close',
}

const zh: Catalog = {
  title: '奇思妙想',
  editorPlaceholder: '有什么灵感?',
  save: '保存',
  saved: '已保存',
  delegate: '委托给 agent',
  delegateDeferred: '委托功能待 agent 接口就绪,暂不可用。',
  delegating: '委托中…',
  waitHint: '这可能要等一会儿 —— 你可以继续写。',
  settings: '设置',
  ideaDir: '想法目录',
  history: '历史',
  statusDraft: '草稿',
  statusRunning: '进行中',
  statusDone: '已完成',
  statusFailed: '失败',
  openResult: '打开结果',
  retry: '重试',
  needVault: '请先打开一个 vault。',
  agentMissing: '未找到 Claude 智能体插件',
  agentMissingHint: '请从插件市场安装 Claude 智能体插件以启用委托。',
  celebrate: '灵感不错!',
  templateH1: '新想法',
  templateHint: '能填多少填多少 —— 剩下的交给 agent 去论证。',
  sectionDomain: '领域',
  sectionTransfer: '迁移',
  sectionResources: '资源',
  sectionOutcome: '结果',
  deleteConfirm: '删除这个想法?',
  close: '关闭',
}

const ja: Catalog = {
  title: 'アイデアスパーク',
  editorPlaceholder: 'ひらめきは何ですか?',
  save: '保存',
  saved: '保存しました',
  delegate: 'エージェントに委任',
  delegateDeferred: '委任機能はまだ準備中です —— エージェント側の対応待ちです。',
  delegating: '委任中…',
  waitHint: '少し時間がかかることがあります —— そのまま書き続けて構いません。',
  settings: '設定',
  ideaDir: 'アイデアフォルダ',
  history: '履歴',
  statusDraft: '下書き',
  statusRunning: '実行中',
  statusDone: '完了',
  statusFailed: '失敗',
  openResult: '結果を開く',
  retry: '再試行',
  needVault: '先に vault を開いてください。',
  agentMissing: 'Claude エージェント プラグインが見つかりません',
  agentMissingHint: '委任を有効にするには、マーケットプレイスから Claude エージェント プラグインをインストールしてください。',
  celebrate: 'いいひらめきですね!',
  templateH1: '新しいアイデア',
  templateHint: '書けるところまで書いてください —— 残りはエージェントが論じます。',
  sectionDomain: '領域',
  sectionTransfer: '転用',
  sectionResources: 'リソース',
  sectionOutcome: '成果',
  deleteConfirm: 'このアイデアを削除しますか?',
  close: '閉じる',
}

const de: Catalog = {
  title: 'Ideenfunke',
  editorPlaceholder: 'Was ist der Funke?',
  save: 'Speichern',
  saved: 'Gespeichert',
  delegate: 'An Agent delegieren',
  delegateDeferred: 'Die Delegation ist noch nicht angebunden — kommt in einem späteren Update.',
  delegating: 'Delegiere…',
  waitHint: 'Das kann eine Weile dauern — du kannst währenddessen weiterschreiben.',
  settings: 'Einstellungen',
  ideaDir: 'Ideenordner',
  history: 'Verlauf',
  statusDraft: 'Entwurf',
  statusRunning: 'Läuft',
  statusDone: 'Fertig',
  statusFailed: 'Fehlgeschlagen',
  openResult: 'Ergebnis öffnen',
  retry: 'Erneut versuchen',
  needVault: 'Öffne zuerst einen Vault.',
  agentMissing: 'Claude-Agent-Plugin nicht gefunden',
  agentMissingHint: 'Installiere das Claude-Agent-Plugin aus dem Marktplatz, um die Delegation zu aktivieren.',
  celebrate: 'Guter Funke!',
  templateH1: 'Neue Idee',
  templateHint: 'Fülle aus, was du kannst — den Rest argumentiert der Agent.',
  sectionDomain: 'Domäne',
  sectionTransfer: 'Übertragung',
  sectionResources: 'Ressourcen',
  sectionOutcome: 'Ergebnis',
  deleteConfirm: 'Diese Idee löschen?',
  close: 'Schließen',
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

// Exported for tests only (catalog completeness checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
