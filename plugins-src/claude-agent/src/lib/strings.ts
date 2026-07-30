// The plugin carries its own string table — a plugin window can't import the
// host's i18n. Missing keys fall back to English.
type Dict = Record<string, string>

const en: Dict = {
  'history.delete': 'Delete this run',
  'history.clearAll': 'Clear all runs',
  'log.back': 'Back to live',
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
  'run.prompt.placeholder': 'What should Claude do this run? (optional)',
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
  'turns': '{n} turns',
}

const zh: Dict = {
  'history.delete': '删除这条运行',
  'history.clearAll': '清空全部运行',
  'log.back': '回到实时',
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
  'run.prompt.placeholder': '这次要 Claude 做什么?(可留空)',
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
  'turns': '{n} 轮',
}

const ja: Dict = {
  'history.delete': 'この実行を削除',
  'history.clearAll': '実行履歴をすべて消去',
  'log.back': 'ライブに戻る',
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
  'run.prompt.placeholder': '今回 Claude に何をさせますか?(任意)',
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
  'turns': '{n} ターン',
}

const de: Dict = {
  'history.delete': 'Diesen Lauf löschen',
  'history.clearAll': 'Alle Läufe löschen',
  'log.back': 'Zurück zum Live-Lauf',
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
  'run.prompt.placeholder': 'Was soll Claude diesmal tun? (optional)',
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
  'turns': '{n} Runden',
}

/** Every locale this window ships. English is the source of truth. */
export const LOCALES = ['en', 'zh', 'ja', 'de'] as const
export type Locale = (typeof LOCALES)[number]

export const CATALOGS: Record<Locale, Dict> = { en, zh, ja, de }

function catalogFor(locale: string): Dict {
  // The host hands us codes like 'zh' or 'zh-CN'; match on the language part.
  const lang = (locale ?? '').slice(0, 2) as Locale
  return CATALOGS[lang] ?? en
}

export function t(locale: string, key: string, vars?: Record<string, string | number>): string {
  let s = catalogFor(locale)[key] ?? en[key] ?? key
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, String(v))
  return s
}
