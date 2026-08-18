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
  'save',
  'saved',
  'saving',
  'saveFailed',
  'delegate',
  'delegating',
  'delegateEmpty',
  'delegateBusy',
  'delegateFailed',
  'notifyOk',
  'notifyFail',
  'notifyDirectiveOk',
  'notifyDirectiveFail',
  'waitHint',
  'settings',
  'ideaDir',
  'inbox',
  'hideInbox',
  'statusDraft',
  'statusRunning',
  'statusDone',
  'statusFailed',
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
  'editorMode',
  'editorUnavailable',
  'unsavedWarning',
  'historyUnavailable',
  'menuDelegate',
  'menuOpenInMain',
  'menuOpenIdea',
  'menuOpenProof',
  'menuRename',
  'menuDelete',
  'renameEmpty',
  'renameSlash',
  'renameDot',
  'renameTaken',
  'renameHint',
  'confirmDeleteTitle',
  'confirmDeleteBody',
  'confirmDelete',
  'cancel',
  'inboxEmpty',
] as const

export type MessageKey = (typeof MESSAGE_KEYS)[number]

type Catalog = Record<MessageKey, string>

// English is the baseline catalog: every other locale is checked against it
// (see strings.test.ts) for key coverage.
const en: Catalog = {
  save: 'Save',
  saved: 'Saved',
  saving: 'Saving…',
  saveFailed: 'Save failed',
  delegate: 'Delegate to agent',
  delegating: 'Delegating…',
  delegateEmpty: 'Write something down before delegating it.',
  // Not a preference: claude-agent locks the task for the length of a run, so
  // a second one would be refused after it had already looked started.
  delegateBusy: 'An idea is already being argued — the agent takes one at a time.',
  delegateFailed: 'The agent couldn’t argue this idea.',
  // The two tray reminders claude-agent pushes when the run ends. The idea's
  // own title is appended to both — a notification arriving an hour later has
  // to say *which* idea it is about.
  notifyOk: 'Idea argued',
  notifyFail: 'Arguing the idea failed',
  notifyDirectiveOk: 'Delegation done',
  notifyDirectiveFail: 'Delegation failed',
  waitHint: 'This can take a minute — feel free to keep writing.',
  settings: 'Settings',
  ideaDir: 'Idea folder',
  inbox: 'Inbox',
  hideInbox: 'Hide inbox',
  statusDraft: 'Draft',
  statusRunning: 'Running',
  statusDone: 'Done',
  statusFailed: 'Failed',
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
  editorMode: 'Editor mode',
  editorUnavailable: 'The rich editor could not be loaded — falling back to plain text.',
  unsavedWarning: 'This idea has unsaved changes.',
  historyUnavailable: 'The idea folder could not be read — the inbox may be incomplete.',
  menuDelegate: 'Delegate to agent',
  menuOpenInMain: 'Open in the main editor',
  menuOpenIdea: 'Open the idea in the main editor',
  menuOpenProof: 'Open the argument in the main editor',
  menuRename: 'Rename',
  menuDelete: 'Delete',
  renameEmpty: 'A name can’t be empty.',
  renameSlash: 'A name can’t contain “/”.',
  renameDot: 'A name can’t start with “.”.',
  renameTaken: 'That name is already taken.',
  renameHint: 'Enter to rename, Esc to cancel',
  confirmDeleteTitle: 'Delete this idea?',
  confirmDeleteBody: 'This deletes the following files for good — there is no trash:',
  confirmDelete: 'Delete',
  cancel: 'Cancel',
  inboxEmpty: 'No ideas yet.',
}

const zh: Catalog = {
  save: '保存',
  saved: '已保存',
  saving: '保存中…',
  saveFailed: '保存失败',
  delegate: '委托给 agent',
  delegating: '委托中…',
  delegateEmpty: '先写点什么,再交给 agent 论证。',
  delegateBusy: '已有一条论证在跑 —— agent 一次只论证一个想法。',
  delegateFailed: 'agent 没能论证这个想法。',
  notifyOk: '论证完成',
  notifyFail: '论证失败',
  notifyDirectiveOk: '委托完成',
  notifyDirectiveFail: '委托失败',
  waitHint: '这可能要等一会儿 —— 你可以继续写。',
  settings: '设置',
  ideaDir: '想法目录',
  inbox: '收件箱',
  hideInbox: '隐藏收件箱',
  statusDraft: '草稿',
  statusRunning: '进行中',
  statusDone: '已完成',
  statusFailed: '失败',
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
  editorMode: '编辑模式',
  editorUnavailable: '富文本编辑器加载失败 —— 已退回纯文本编辑。',
  unsavedWarning: '这个想法还有未保存的修改。',
  historyUnavailable: '读取想法目录失败 —— 收件箱可能不完整。',
  menuDelegate: '委托给 agent',
  menuOpenInMain: '在主编辑器打开',
  menuOpenIdea: '在主编辑器打开想法',
  menuOpenProof: '在主编辑器打开论证',
  menuRename: '重命名',
  menuDelete: '删除',
  renameEmpty: '名字不能为空。',
  renameSlash: '名字不能包含「/」。',
  renameDot: '名字不能以「.」开头。',
  renameTaken: '这个名字已被占用。',
  renameHint: '回车重命名,Esc 取消',
  confirmDeleteTitle: '删除这个想法?',
  confirmDeleteBody: '以下文件会被彻底删除,没有回收站:',
  confirmDelete: '删除',
  cancel: '取消',
  inboxEmpty: '还没有想法。',
}

const ja: Catalog = {
  save: '保存',
  saved: '保存しました',
  saving: '保存中…',
  saveFailed: '保存に失敗しました',
  delegate: 'エージェントに委任',
  delegating: '委任中…',
  delegateEmpty: '何か書いてから委任してください。',
  delegateBusy: 'すでに 1 件の論証が実行中です —— エージェントは一度に 1 件だけ扱います。',
  delegateFailed: 'エージェントはこのアイデアを論証できませんでした。',
  notifyOk: '論証が完了しました',
  notifyFail: '論証に失敗しました',
  notifyDirectiveOk: '委任が完了しました',
  notifyDirectiveFail: '委任に失敗しました',
  waitHint: '少し時間がかかることがあります —— そのまま書き続けて構いません。',
  settings: '設定',
  ideaDir: 'アイデアフォルダ',
  inbox: 'インボックス',
  hideInbox: 'インボックスを隠す',
  statusDraft: '下書き',
  statusRunning: '実行中',
  statusDone: '完了',
  statusFailed: '失敗',
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
  editorMode: 'エディターモード',
  editorUnavailable: 'リッチエディタを読み込めませんでした —— プレーンテキストに切り替えます。',
  unsavedWarning: 'このアイデアには未保存の変更があります。',
  historyUnavailable: 'アイデアフォルダを読み取れませんでした —— インボックスが不完全な可能性があります。',
  menuDelegate: 'エージェントに委任',
  menuOpenInMain: 'メインエディタで開く',
  menuOpenIdea: 'アイデアをメインエディタで開く',
  menuOpenProof: '論証をメインエディタで開く',
  menuRename: '名前を変更',
  menuDelete: '削除',
  renameEmpty: '名前は空にできません。',
  renameSlash: '名前に「/」は使えません。',
  renameDot: '名前を「.」で始めることはできません。',
  renameTaken: 'この名前はすでに使われています。',
  renameHint: 'Enter で変更、Esc で取消',
  confirmDeleteTitle: 'このアイデアを削除しますか?',
  confirmDeleteBody: '次のファイルを完全に削除します(ごみ箱はありません):',
  confirmDelete: '削除',
  cancel: 'キャンセル',
  inboxEmpty: 'まだアイデアがありません。',
}

const de: Catalog = {
  save: 'Speichern',
  saved: 'Gespeichert',
  saving: 'Speichere…',
  saveFailed: 'Speichern fehlgeschlagen',
  delegate: 'An Agent delegieren',
  delegating: 'Delegiere…',
  delegateEmpty: 'Schreib erst etwas auf, bevor du es delegierst.',
  delegateBusy: 'Eine Idee wird bereits durchargumentiert — der Agent nimmt eine nach der anderen.',
  delegateFailed: 'Der Agent konnte diese Idee nicht durchargumentieren.',
  notifyOk: 'Idee durchargumentiert',
  notifyFail: 'Argumentation fehlgeschlagen',
  notifyDirectiveOk: 'Auftrag erledigt',
  notifyDirectiveFail: 'Auftrag fehlgeschlagen',
  waitHint: 'Das kann eine Weile dauern — du kannst währenddessen weiterschreiben.',
  settings: 'Einstellungen',
  ideaDir: 'Ideenordner',
  inbox: 'Eingang',
  hideInbox: 'Eingang ausblenden',
  statusDraft: 'Entwurf',
  statusRunning: 'Läuft',
  statusDone: 'Fertig',
  statusFailed: 'Fehlgeschlagen',
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
  editorMode: 'Editormodus',
  editorUnavailable: 'Der Rich-Text-Editor konnte nicht geladen werden — Rückfall auf reinen Text.',
  unsavedWarning: 'Diese Idee hat ungespeicherte Änderungen.',
  historyUnavailable: 'Der Ideenordner konnte nicht gelesen werden — der Eingang ist womöglich unvollständig.',
  menuDelegate: 'An Agent delegieren',
  menuOpenInMain: 'Im Haupteditor öffnen',
  menuOpenIdea: 'Idee im Haupteditor öffnen',
  menuOpenProof: 'Begründung im Haupteditor öffnen',
  menuRename: 'Umbenennen',
  menuDelete: 'Löschen',
  renameEmpty: 'Ein Name darf nicht leer sein.',
  renameSlash: 'Ein Name darf kein „/“ enthalten.',
  renameDot: 'Ein Name darf nicht mit „.“ beginnen.',
  renameTaken: 'Dieser Name ist bereits vergeben.',
  renameHint: 'Enter zum Umbenennen, Esc zum Abbrechen',
  confirmDeleteTitle: 'Diese Idee löschen?',
  confirmDeleteBody: 'Die folgenden Dateien werden endgültig gelöscht — es gibt keinen Papierkorb:',
  confirmDelete: 'Löschen',
  cancel: 'Abbrechen',
  inboxEmpty: 'Noch keine Ideen.',
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
 * The locale `setLocale` settled on. Exported for the Intl formatters the UI
 * builds itself (relative times in the inbox) — those need a locale code, not a
 * catalog lookup, and must not guess one from `navigator.language` when the
 * host has already resolved what language this window is speaking.
 */
export function locale(): Locale {
  return active
}

// Exported for tests only (catalog completeness checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
