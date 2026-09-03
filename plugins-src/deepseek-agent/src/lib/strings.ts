// The plugin carries its own string table — a plugin window can't import the
// host's i18n. Missing keys fall back to English.

export type MessageKey =
  | 'run.addendum'
  | 'run.addendum.placeholder'
  | 'run.willRun'
  | 'history.delete'
  | 'history.clearAll'
  | 'log.empty'
  | 'status.skipped'
  | 'artifacts.label'
  | 'history.all'
  | 'history.thisTask'
  | 'history.emptyAll'
  | 'tasks.title'
  | 'tasks.empty'
  | 'tasks.group.core'
  | 'tasks.group.ebook'
  | 'tasks.group.idea'
  | 'tasks.group.memory'
  | 'tasks.group.trace'
  | 'tasks.group.agent'
  | 'tasks.group.custom'
  | 'run.start'
  | 'run.stop'
  | 'ctx.label'
  | 'ctx.selection'
  | 'history.title'
  | 'history.empty'
  | 'status.running'
  | 'status.success'
  | 'status.error'
  | 'status.timeout'
  | 'status.cancelled'
  | 'status.busy'
  | 'turns'
  | 'agentPicker.by'
  | 'harness.probing'
  | 'harness.unknown'
  | 'harness.missing'
  | 'harness.broken'
  | 'harness.model'
  | 'harness.warning'
  | 'settings.title'
  | 'settings.maxConcurrency'
  | 'settings.maxConcurrencyHint'
  | 'settings.usageDisplay'
  | 'settings.usageDisplayHint'
  | 'settings.usageDisplayTip'
  | 'settings.usageDisplayResult'
  | 'usage.total'
  | 'usage.input'
  | 'usage.cacheRead'
  | 'usage.cacheWrite'
  | 'usage.output'
  | 'usage.reasoning'
  | 'usage.costReported'
  | 'usage.costEstimated'
  | 'usage.costDisclaimer'
  | 'usage.unavailable'
  | 'settings.loadFailed'
  | 'settings.saveFailed'
  | 'err.noVault'
  | 'err.harnessNotFound'
  | 'err.badPolicy'
  | 'err.unknownTask'
  | 'err.notRunning'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  'run.addendum': 'Add to this run (optional)',
  'run.addendum.placeholder': 'e.g. only the questions about performance',
  'run.willRun': 'Will run',
  'history.delete': 'Delete this run',
  'history.clearAll': 'Clear all runs',
  'log.empty': 'This run left no log.',
  'status.skipped': 'Skipped',
  'artifacts.label': 'Opens:',
  'history.all': 'All tasks',
  'history.thisTask': 'This task',
  'history.emptyAll': 'No task has run yet.',
  'tasks.title': 'Tasks',
  'tasks.empty': 'No task templates yet.',
  'tasks.group.core': 'note.md',
  'tasks.group.ebook': 'Ebook Import',
  'tasks.group.idea': 'Idea Spark',
  'tasks.group.memory': 'Memory',
  'tasks.group.trace': 'Trace Source',
  'tasks.group.agent': 'Agent tools',
  'tasks.group.custom': 'Custom tasks',
  'run.start': 'Run',
  'run.stop': 'Stop',
  'ctx.label': 'Context',
  'ctx.selection': '{n} chars selected',
  'history.title': 'Recent runs',
  'history.empty': 'Nothing has run yet.',
  'status.running': 'Running',
  'status.success': 'Success',
  'status.error': 'Failed',
  'status.timeout': 'Timed out',
  'status.cancelled': 'Stopped',
  'status.busy': 'Already running',
  turns: '{n} turns',
  'agentPicker.by': 'by {name}',
  'harness.probing': 'Checking the harness…',
  'harness.unknown': 'Could not read the harness status — restart the app after updating this plugin.',
  'harness.missing': 'not installed',
  'harness.broken': 'found, but it will not start',
  'harness.model': 'model {model}',
  'harness.warning': 'Last run failed on the environment: {detail}',
  'settings.title': 'Settings',
  'settings.maxConcurrency': 'Maximum concurrent AI reads',
  'settings.maxConcurrencyHint': 'Limits this agent in ebook “AI pre-read” queues. Lowering it does not interrupt work already running.',
  'settings.usageDisplay': 'Show token usage after a run',
  'settings.usageDisplayHint': 'Usage is always saved in run history. Choose where the completion summary appears.',
  'settings.usageDisplayTip': 'Completion tip',
  'settings.usageDisplayResult': 'Result area',
  'usage.total': '{n} tokens',
  'usage.input': 'input {n}',
  'usage.cacheRead': 'cache read {n}',
  'usage.cacheWrite': 'cache write {n}',
  'usage.output': 'output {n}',
  'usage.reasoning': 'reasoning {n}',
  'usage.costReported': '${amount} reported',
  'usage.costEstimated': 'API list-price estimate ≈${amount}',
  'usage.costDisclaimer': 'API list-price estimate; it may differ from subscription usage or the actual bill.',
  'usage.unavailable': 'Token usage was not reported by this harness.',
  'settings.loadFailed': 'Could not load this setting. The default is 1.',
  'settings.saveFailed': 'Could not save this setting. The previous value is still active.',
  'err.noVault': 'No vault configured — open or create a vault first.',
  'err.harnessNotFound':
    'DeepSeek Harness ACP server not found — install @deepseek-ai/dsh-acp-demo, or set dsh_acp_bin / DSH_REPO.',
  'err.badPolicy': "This task's policy.json could not be read — fix it before running.",
  'err.unknownTask': 'This task no longer exists — pick another one.',
  'err.notRunning': 'That run has already finished.',
}

const zh: Catalog = {
  'run.addendum': '本次补充要求(可选)',
  'run.addendum.placeholder': '例:只回答与性能有关的问题',
  'run.willRun': '将运行',
  'history.delete': '删除这条运行',
  'history.clearAll': '清空全部运行',
  'log.empty': '这次运行没有日志。',
  'status.skipped': '已跳过',
  'artifacts.label': '产物:',
  'history.all': '全部任务',
  'history.thisTask': '当前任务',
  'history.emptyAll': '还没有任何任务跑过。',
  'tasks.title': '任务',
  'tasks.empty': '还没有任务模板。',
  'tasks.group.core': 'note.md 核心',
  'tasks.group.ebook': '导入电子书',
  'tasks.group.idea': '奇思妙想',
  'tasks.group.memory': '记忆',
  'tasks.group.trace': '溯源',
  'tasks.group.agent': 'Agent 工具',
  'tasks.group.custom': '自定义任务',
  'run.start': '运行',
  'run.stop': '停止',
  'ctx.label': '上下文',
  'ctx.selection': '选中 {n} 字',
  'history.title': '最近运行',
  'history.empty': '还没有跑过。',
  'status.running': '运行中',
  'status.success': '成功',
  'status.error': '失败',
  'status.timeout': '超时',
  'status.cancelled': '已停止',
  'status.busy': '已在运行',
  turns: '{n} 轮',
  'agentPicker.by': '由 {name} 执行',
  'harness.probing': '正在检查运行环境…',
  'harness.unknown': '读不到运行环境状态 —— 插件更新后请重启应用。',
  'harness.missing': '未安装',
  'harness.broken': '装了,但起不来',
  'harness.model': '模型 {model}',
  'harness.warning': '上次运行因环境问题失败:{detail}',
  'settings.title': '设置',
  'settings.maxConcurrency': 'AI 阅读最大并行数',
  'settings.maxConcurrencyHint': '限制此智能体在电子书“AI 先读”队列中的并行数。调低后不会中断正在运行的任务。',
  'settings.usageDisplay': '运行后显示 Token 用量',
  'settings.usageDisplayHint': '用量始终保存在运行历史中；这里选择完成摘要的显示位置。',
  'settings.usageDisplayTip': '完成提示',
  'settings.usageDisplayResult': '结果区',
  'usage.total': '共 {n} tokens',
  'usage.input': '输入 {n}',
  'usage.cacheRead': '缓存读取 {n}',
  'usage.cacheWrite': '缓存写入 {n}',
  'usage.output': '输出 {n}',
  'usage.reasoning': '推理 {n}',
  'usage.costReported': '报告费用 ${amount}',
  'usage.costEstimated': 'API 官网标价估算约 ${amount}',
  'usage.costDisclaimer': '按 API 标价估算，可能与订阅额度或实际账单不同。',
  'usage.unavailable': '当前运行框架未报告 Token 用量。',
  'settings.loadFailed': '无法读取此设置，将使用默认值 1。',
  'settings.saveFailed': '无法保存此设置，仍使用之前的值。',
  'err.noVault': '未配置 vault——请先打开或创建一个 vault。',
  'err.harnessNotFound':
    '未找到 DeepSeek Harness 的 ACP 服务端——请装 @deepseek-ai/dsh-acp-demo,或设置 dsh_acp_bin / DSH_REPO。',
  'err.badPolicy': '这个任务的 policy.json 读不出来——先修好再运行。',
  'err.unknownTask': '这个任务已不存在——请换一个。',
  'err.notRunning': '这次运行已经结束。',
}

const ja: Catalog = {
  'run.addendum': '今回の追加指示(任意)',
  'run.addendum.placeholder': '例:性能に関する質問だけ',
  'run.willRun': '実行内容',
  'history.delete': 'この実行を削除',
  'history.clearAll': '実行履歴をすべて消去',
  'log.empty': 'この実行にログはありません。',
  'status.skipped': 'スキップ',
  'artifacts.label': '成果物:',
  'history.all': 'すべてのタスク',
  'history.thisTask': 'このタスク',
  'history.emptyAll': 'まだどのタスクも実行されていません。',
  'tasks.title': 'タスク',
  'tasks.empty': 'タスクテンプレートがまだありません。',
  'tasks.group.core': 'note.md コア',
  'tasks.group.ebook': '電子書籍を取り込む',
  'tasks.group.idea': 'アイデアスパーク',
  'tasks.group.memory': 'メモリ',
  'tasks.group.trace': '出典をたどる',
  'tasks.group.agent': 'Agent ツール',
  'tasks.group.custom': 'カスタムタスク',
  'run.start': '実行',
  'run.stop': '停止',
  'ctx.label': 'コンテキスト',
  'ctx.selection': '{n} 文字選択中',
  'history.title': '最近の実行',
  'history.empty': 'まだ実行されていません。',
  'status.running': '実行中',
  'status.success': '成功',
  'status.error': '失敗',
  'status.timeout': 'タイムアウト',
  'status.cancelled': '停止しました',
  'status.busy': '実行中です',
  turns: '{n} ターン',
  'agentPicker.by': '実行:{name}',
  'harness.probing': '実行環境を確認中…',
  'harness.unknown': '実行環境の状態を取得できません。プラグイン更新後はアプリを再起動してください。',
  'harness.missing': '未インストール',
  'harness.broken': 'インストール済みですが起動できません',
  'harness.model': 'モデル {model}',
  'harness.warning': '前回の実行は環境の問題で失敗しました:{detail}',
  'settings.title': '設定',
  'settings.maxConcurrency': 'AI 読書の最大同時実行数',
  'settings.maxConcurrencyHint': '電子書籍の「AI 先読み」キューで、このエージェントの同時実行数を制限します。値を下げても実行中の処理は中断されません。',
  'settings.usageDisplay': '実行後のトークン使用量',
  'settings.usageDisplayHint': '使用量は常に履歴へ保存されます。完了サマリーの表示場所を選びます。',
  'settings.usageDisplayTip': '完了通知',
  'settings.usageDisplayResult': '結果エリア',
  'usage.total': '合計 {n} tokens',
  'usage.input': '入力 {n}',
  'usage.cacheRead': 'キャッシュ読込 {n}',
  'usage.cacheWrite': 'キャッシュ書込 {n}',
  'usage.output': '出力 {n}',
  'usage.reasoning': '推論 {n}',
  'usage.costReported': '報告コスト ${amount}',
  'usage.costEstimated': 'API 定価見積り 約 ${amount}',
  'usage.costDisclaimer': 'API 定価による見積りです。契約枠や実際の請求とは異なる場合があります。',
  'usage.unavailable': 'この実行環境はトークン使用量を報告しませんでした。',
  'settings.loadFailed': 'この設定を読み込めません。既定値 1 を使用します。',
  'settings.saveFailed': 'この設定を保存できません。以前の値が引き続き有効です。',
  'err.noVault': 'vault が未設定です。まず vault を開くか作成してください。',
  'err.harnessNotFound':
    'DeepSeek Harness の ACP サーバーが見つかりません。@deepseek-ai/dsh-acp-demo をインストールするか、dsh_acp_bin / DSH_REPO を設定してください。',
  'err.badPolicy': 'このタスクの policy.json を読み取れません。修正してから実行してください。',
  'err.unknownTask': 'このタスクはもう存在しません。ほかのタスクを選んでください。',
  'err.notRunning': 'この実行はすでに終了しています。',
}

const de: Catalog = {
  'run.addendum': 'Für diesen Lauf ergänzen (optional)',
  'run.addendum.placeholder': 'z. B. nur die Fragen zur Performance',
  'run.willRun': 'Läuft',
  'history.delete': 'Diesen Lauf löschen',
  'history.clearAll': 'Alle Läufe löschen',
  'log.empty': 'Dieser Lauf hat kein Protokoll.',
  'status.skipped': 'Übersprungen',
  'artifacts.label': 'Ergebnis:',
  'history.all': 'Alle Aufgaben',
  'history.thisTask': 'Diese Aufgabe',
  'history.emptyAll': 'Noch keine Aufgabe gelaufen.',
  'tasks.title': 'Aufgaben',
  'tasks.empty': 'Noch keine Aufgabenvorlagen.',
  'tasks.group.core': 'note.md-Kern',
  'tasks.group.ebook': 'E-Books importieren',
  'tasks.group.idea': 'Ideenfunke',
  'tasks.group.memory': 'Gedächtnis',
  'tasks.group.trace': 'Quellensuche',
  'tasks.group.agent': 'Agent-Werkzeuge',
  'tasks.group.custom': 'Eigene Aufgaben',
  'run.start': 'Ausführen',
  'run.stop': 'Stoppen',
  'ctx.label': 'Kontext',
  'ctx.selection': '{n} Zeichen ausgewählt',
  'history.title': 'Letzte Läufe',
  'history.empty': 'Noch nichts gelaufen.',
  'status.running': 'Läuft',
  'status.success': 'Erfolgreich',
  'status.error': 'Fehlgeschlagen',
  'status.timeout': 'Zeitüberschreitung',
  'status.cancelled': 'Gestoppt',
  'status.busy': 'Läuft bereits',
  turns: '{n} Runden',
  'agentPicker.by': 'via {name}',
  'harness.probing': 'Umgebung wird geprüft…',
  'harness.unknown': 'Umgebungsstatus nicht lesbar — App nach dem Plugin-Update neu starten.',
  'harness.missing': 'nicht installiert',
  'harness.broken': 'vorhanden, startet aber nicht',
  'harness.model': 'Modell {model}',
  'harness.warning': 'Letzter Lauf scheiterte an der Umgebung: {detail}',
  'settings.title': 'Einstellungen',
  'settings.maxConcurrency': 'Maximale parallele KI-Lesevorgänge',
  'settings.maxConcurrencyHint': 'Begrenzt diesen Agenten in den „KI-Vorlesen“-Warteschlangen für E-Books. Eine Verringerung unterbricht keine laufende Aufgabe.',
  'settings.usageDisplay': 'Tokenverbrauch nach einem Lauf',
  'settings.usageDisplayHint': 'Der Verbrauch wird immer im Verlauf gespeichert. Hier wird die Anzeige des Abschlusses gewählt.',
  'settings.usageDisplayTip': 'Abschlusshinweis',
  'settings.usageDisplayResult': 'Ergebnisbereich',
  'usage.total': '{n} Tokens',
  'usage.input': 'Eingabe {n}',
  'usage.cacheRead': 'Cache gelesen {n}',
  'usage.cacheWrite': 'Cache geschrieben {n}',
  'usage.output': 'Ausgabe {n}',
  'usage.reasoning': 'Reasoning {n}',
  'usage.costReported': '${amount} gemeldet',
  'usage.costEstimated': 'API-Listenpreisschätzung ca. ${amount}',
  'usage.costDisclaimer': 'Schätzung nach API-Listenpreis; Abo-Nutzung und tatsächliche Rechnung können abweichen.',
  'usage.unavailable': 'Diese Laufzeit hat keinen Tokenverbrauch gemeldet.',
  'settings.loadFailed': 'Diese Einstellung konnte nicht geladen werden. Standardwert 1 wird verwendet.',
  'settings.saveFailed': 'Diese Einstellung konnte nicht gespeichert werden. Der vorherige Wert bleibt aktiv.',
  'err.noVault': 'Kein Vault konfiguriert — bitte zuerst einen Vault öffnen oder erstellen.',
  'err.harnessNotFound':
    'DeepSeek-Harness-ACP-Server nicht gefunden — @deepseek-ai/dsh-acp-demo installieren oder dsh_acp_bin / DSH_REPO setzen.',
  'err.badPolicy':
    'Die policy.json dieser Aufgabe konnte nicht gelesen werden — bitte vor dem Ausführen korrigieren.',
  'err.unknownTask': 'Diese Aufgabe gibt es nicht mehr — bitte eine andere wählen.',
  'err.notRunning': 'Dieser Lauf ist bereits beendet.',
}

/** Every locale this window ships. English is the source of truth. */
export const LOCALES = ['en', 'zh', 'ja', 'de'] as const
export type Locale = (typeof LOCALES)[number]

export const CATALOGS: Record<Locale, Catalog> = { en, zh, ja, de }

function catalogFor(locale: string): Catalog {
  // The host hands us codes like 'zh' or 'zh-CN'; match on the language part.
  const lang = (locale ?? '').slice(0, 2) as Locale
  return CATALOGS[lang] ?? en
}

export function t(locale: string, key: MessageKey, vars?: Record<string, string | number>): string {
  let s = catalogFor(locale)[key] ?? en[key] ?? key
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, String(v))
  return s
}
