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
  | 'harness.probing'
  | 'harness.missing'
  | 'harness.model'
  | 'harness.warning'
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
  'harness.probing': 'Checking the harness…',
  'harness.missing': 'not installed',
  'harness.model': 'model {model}',
  'harness.warning': 'Last run failed on the environment: {detail}',
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
  'harness.probing': '正在检查运行环境…',
  'harness.missing': '未安装',
  'harness.model': '模型 {model}',
  'harness.warning': '上次运行因环境问题失败:{detail}',
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
  'harness.probing': '実行環境を確認中…',
  'harness.missing': '未インストール',
  'harness.model': 'モデル {model}',
  'harness.warning': '前回の実行は環境の問題で失敗しました:{detail}',
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
  'harness.probing': 'Umgebung wird geprüft…',
  'harness.missing': 'nicht installiert',
  'harness.model': 'Modell {model}',
  'harness.warning': 'Letzter Lauf scheiterte an der Umgebung: {detail}',
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
