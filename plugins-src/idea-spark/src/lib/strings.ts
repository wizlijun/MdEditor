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
  'ph1',
  'ph2',
  'ph3',
  'ph4',
  'ph5',
  'close',
  'newIdea',
  'modeRich',
  'modeSource',
  'editorUnavailable',
  'unsavedWarning',
  'historyUnavailable',
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
  ph1: 'Three rules for writing a novel — unfortunately, nobody knows what they are — Maugham',
  ph2: 'Ideas are like rabbits — get a couple and soon you have a dozen — Steinbeck',
  ph3: 'Writing is easy, you just stare at a blank page until your forehead bleeds — Gene Fowler',
  ph4: 'Inspiration is for amateurs — Chuck Close',
  ph5: 'I made this longer because I had no time to make it shorter — Pascal',
  close: 'Close',
  newIdea: 'New idea',
  modeRich: 'Formatted',
  modeSource: 'Markdown source',
  editorUnavailable: 'The rich editor could not be loaded — falling back to plain text.',
  unsavedWarning: 'This idea has unsaved changes.',
  historyUnavailable: 'The idea folder could not be read — the history may be incomplete.',
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
  ph1: '写小说有三条规矩,可惜没人知道是哪三条 —— 毛姆',
  ph2: '想法像兔子,养两只很快就一打 —— 斯坦贝克',
  ph3: '写作很简单,盯着白纸直到额头渗出血珠 —— 吉恩·福勒',
  ph4: '灵感是业余选手的事 —— 查克·克洛斯',
  ph5: '这封信写长了,因为我没时间把它写短 —— 帕斯卡',
  close: '关闭',
  newIdea: '新想法',
  modeRich: '排版视图',
  modeSource: 'Markdown 源码',
  editorUnavailable: '富文本编辑器加载失败 —— 已退回纯文本编辑。',
  unsavedWarning: '这个想法还有未保存的修改。',
  historyUnavailable: '读取想法目录失败 —— 历史列表可能不完整。',
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
  ph1: '小説の書き方には三つの規則がある、残念ながら誰も知らない —— モーム',
  ph2: 'アイデアはウサギに似ている、二匹いればすぐ一ダース —— スタインベック',
  ph3: '書くのは簡単、額から血がにじむまで白紙を見つめるだけ —— ジーン・ファウラー',
  ph4: 'ひらめきは素人のもの —— チャック・クローズ',
  ph5: '短くする時間がなかったので長くなった —— パスカル',
  close: '閉じる',
  newIdea: '新しいアイデア',
  modeRich: 'リッチ表示',
  modeSource: 'Markdown ソース',
  editorUnavailable: 'リッチエディタを読み込めませんでした —— プレーンテキストに切り替えます。',
  unsavedWarning: 'このアイデアには未保存の変更があります。',
  historyUnavailable: 'アイデアフォルダを読み取れませんでした —— 履歴が不完全な可能性があります。',
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
  ph1: 'Es gibt drei Regeln für einen Roman, leider kennt sie niemand — Maugham',
  ph2: 'Ideen sind wie Kaninchen, zwei werden schnell ein Dutzend — Steinbeck',
  ph3: 'Schreiben ist leicht, starr auf das leere Blatt, bis dir Blut auf der Stirn steht — Gene Fowler',
  ph4: 'Inspiration ist etwas für Amateure — Chuck Close',
  ph5: 'Dieser Brief wurde lang, weil ich keine Zeit hatte, ihn kurz zu machen — Pascal',
  close: 'Schließen',
  newIdea: 'Neue Idee',
  modeRich: 'Formatiert',
  modeSource: 'Markdown-Quelltext',
  editorUnavailable: 'Der Rich-Text-Editor konnte nicht geladen werden — Rückfall auf reinen Text.',
  unsavedWarning: 'Diese Idee hat ungespeicherte Änderungen.',
  historyUnavailable: 'Der Ideenordner konnte nicht gelesen werden — der Verlauf ist womöglich unvollständig.',
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
