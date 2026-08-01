// src/lib/strings.ts — self-contained i18n for the roam-import plugin.
//
// Originally seeded (2026-07) from the host `roamImport.*` catalog entries,
// which were removed from src/lib/i18n/{en,zh,ja,de}.ts when the plugin moved
// to the v2 isolated webview — this file is now the sole source of truth.
// The keys here drop the `roamImport.` prefix (the plugin has no other
// namespace). Language is chosen by `notemd.locale` at startup; English is
// the fallback for any missing key.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'title'
  | 'hint.title'
  | 'hint.step1'
  | 'hint.step2'
  | 'pickFile'
  | 'dialog.filter'
  | 'noVault'
  | 'stage.parse'
  | 'stage.plan'
  | 'stage.write'
  | 'progress'
  | 'errors'
  | 'copyLog'
  | 'done'
  | 'doneErrors'
  | 'conflicts'
  | 'overwriteSelected'
  | 'errParse'
  | 'errWrite'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Import from Roam Research',
  'hint.title': 'Before importing',
  'hint.step1': 'In Roam, use “Export All” and choose the JSON format, or compress the export into a .zip.',
  'hint.step2': 'Keep the export file under 200 MB.',
  pickFile: 'Choose Roam export (.zip / .json)…',
  'dialog.filter': 'Roam export',
  noVault: 'Configure a Vault first (Settings → Vault) to import.',
  'stage.parse': 'Parsing export…',
  'stage.plan': 'Planning import…',
  'stage.write': 'Writing notes…',
  progress: '{done} / {total} pages — {current}',
  errors: 'Errors & warnings',
  copyLog: 'Copy log',
  done: 'Import finished: {wiki} wiki pages, {daily} daily notes, {skipped} skipped.',
  doneErrors: 'Finished with {errors} problem(s) — see log below.',
  conflicts: '{count} page(s) modified locally were skipped.',
  overwriteSelected: 'Overwrite selected',
  errParse: 'Export not readable: {error}',
  errWrite: 'Write failed for {page}: {error}',
}

const zh: Catalog = {
  title: '从 Roam Research 导入',
  'hint.title': '导入前请注意',
  'hint.step1': '在 Roam 中使用「Export All」并选择 JSON 格式，或将导出文件压缩为 .zip。',
  'hint.step2': '导出文件需小于 200 MB。',
  pickFile: '选择 Roam 导出文件（.zip / .json）…',
  'dialog.filter': 'Roam 导出文件',
  noVault: '请先在 设置 → Vault 配置仓库后再导入。',
  'stage.parse': '正在解析导出文件…',
  'stage.plan': '正在计算导入计划…',
  'stage.write': '正在写入笔记…',
  progress: '{done} / {total} 页 — {current}',
  errors: '错误与警告',
  copyLog: '复制日志',
  done: '导入完成：{wiki} 个 wiki 页、{daily} 篇日记、跳过 {skipped}。',
  doneErrors: '完成，但有 {errors} 个问题 — 见下方日志。',
  conflicts: '{count} 个页面因本地已修改被跳过。',
  overwriteSelected: '覆盖所选',
  errParse: '导出文件不可读：{error}',
  errWrite: '写入 {page} 失败：{error}',
}

const ja: Catalog = {
  title: 'Roam Research からインポート',
  'hint.title': 'インポート前に',
  'hint.step1': 'Roam で「Export All」を使い JSON 形式を選択するか、エクスポートを .zip に圧縮してください。',
  'hint.step2': 'エクスポートファイルは 200 MB 未満にしてください。',
  pickFile: 'Roam エクスポートを選択（.zip / .json）…',
  'dialog.filter': 'Roam エクスポート',
  noVault: '先に 設定 → Vault でボールトを設定してください。',
  'stage.parse': 'エクスポートを解析中…',
  'stage.plan': 'インポート計画を作成中…',
  'stage.write': 'ノートを書き込み中…',
  progress: '{done} / {total} ページ — {current}',
  errors: 'エラーと警告',
  copyLog: 'ログをコピー',
  done: '完了：wiki {wiki} 件、デイリー {daily} 件、スキップ {skipped} 件。',
  doneErrors: '完了しましたが {errors} 件の問題があります（下のログ参照）。',
  conflicts: 'ローカルで変更済みの {count} ページをスキップしました。',
  overwriteSelected: '選択を上書き',
  errParse: 'エクスポートを読み込めません：{error}',
  errWrite: '{page} の書き込みに失敗：{error}',
}

const de: Catalog = {
  title: 'Aus Roam Research importieren',
  'hint.title': 'Vor dem Import',
  'hint.step1': 'Nutzen Sie in Roam „Export All“ und wählen Sie das JSON-Format, oder komprimieren Sie den Export als .zip.',
  'hint.step2': 'Die Export-Datei muss kleiner als 200 MB sein.',
  pickFile: 'Roam-Export auswählen (.zip / .json)…',
  'dialog.filter': 'Roam-Export',
  noVault: 'Konfigurieren Sie zuerst einen Tresor (Einstellungen → Tresor), um zu importieren.',
  'stage.parse': 'Export wird analysiert…',
  'stage.plan': 'Import wird geplant…',
  'stage.write': 'Notizen werden geschrieben…',
  progress: '{done} / {total} Seiten — {current}',
  errors: 'Fehler & Warnungen',
  copyLog: 'Protokoll kopieren',
  done: 'Import abgeschlossen: {wiki} Wiki-Seiten, {daily} Tagesnotizen, {skipped} übersprungen.',
  doneErrors: 'Mit {errors} Problem(en) abgeschlossen — siehe Protokoll unten.',
  conflicts: '{count} lokal geänderte Seite(n) wurden übersprungen.',
  overwriteSelected: 'Ausgewählte überschreiben',
  errParse: 'Export nicht lesbar: {error}',
  errWrite: 'Schreiben fehlgeschlagen für {page}: {error}',
}

const registry: Record<Locale, Catalog> = { en, zh, ja, de }

let active: Locale = 'en'

function isLocale(v: unknown): v is Locale {
  return v === 'en' || v === 'zh' || v === 'ja' || v === 'de'
}

/** Set the active locale from `notemd.locale`; unknown/absent → English. */
export function setLocale(code: string | undefined): void {
  active = isLocale(code) ? code : 'en'
}

/**
 * Translate `key`, filling `{name}` placeholders from `params`. Falls back to
 * the English catalog for a missing key, then to the raw key. Mirrors the
 * host's `t()` (src/lib/i18n/store.svelte.ts).
 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const catalog = registry[active] ?? en
  let s = catalog[key] ?? en[key] ?? key
  if (params) {
    s = s.replace(/\{(\w+)\}/g, (m, name) => (name in params ? String(params[name]) : m))
  }
  return s
}

// Exported for tests only (catalog completeness / placeholder parity checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
