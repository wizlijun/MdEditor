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
  'notifyOk',
  'notifyFail',
  'newTrace',
  'placeholder',
  'needVault',
  'agentMissing',
  'agentMissingHint',
  'editorUnavailable',
  'close',
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
  notifyOk: '溯源完成',
  notifyFail: '溯源失败',
  newTrace: '新溯源',
  placeholder: '粘贴要溯源的文字;可补充范围说明,如「只查 YouTube 和 arxiv」。',
  needVault: '请先打开一个 vault。',
  agentMissing: '未找到 agent 插件',
  agentMissingHint: '请从插件市场安装一个 agent 插件(如 Claude 智能体)以启用溯源。',
  editorUnavailable: '富文本编辑器加载失败 —— 已退回纯文本编辑。',
  close: '关闭',
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
  notifyOk: '出典レポートが完成しました',
  notifyFail: '出典の追跡に失敗しました',
  newTrace: '新しい追跡',
  placeholder: 'たどりたい文章を貼り付けてください。「YouTube と arxiv だけ」のような範囲の指定も書けます。',
  needVault: '先に vault を開いてください。',
  agentMissing: 'エージェント プラグインが見つかりません',
  agentMissingHint: '追跡を有効にするには、マーケットプレイスからエージェント プラグイン(例:Claude エージェント)をインストールしてください。',
  editorUnavailable: 'リッチエディタを読み込めませんでした —— プレーンテキストに切り替えます。',
  close: '閉じる',
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
  notifyOk: 'Quellenbericht fertig',
  notifyFail: 'Quellensuche fehlgeschlagen',
  newTrace: 'Neue Suche',
  placeholder: 'Füge die Passage ein, deren Ursprung gesucht werden soll. Eingrenzungen sind willkommen — z. B. „nur YouTube und arxiv“.',
  needVault: 'Öffne zuerst einen Vault.',
  agentMissing: 'Kein Agent-Plugin gefunden',
  agentMissingHint: 'Installiere ein Agent-Plugin (z. B. Claude Agent) aus dem Marktplatz, um die Quellensuche zu aktivieren.',
  editorUnavailable: 'Der Rich-Text-Editor konnte nicht geladen werden — Rückfall auf reinen Text.',
  close: 'Schließen',
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

// Exported for tests only (catalog completeness checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
