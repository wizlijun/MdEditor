// src/lib/strings.ts — self-contained i18n for the ebook-import plugin.
//
// A plugin window can't import the host's i18n store, so this mirrors its
// shape (src/lib/i18n/store.svelte.ts) in miniature: a MessageKey union, one
// catalog per locale, and a `t()` that falls back to English. Language is
// chosen from `notemd.locale` at startup via `setLocale`; see App.svelte.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'title'
  | 'drop.hint'
  | 'drop.pick'
  | 'ocr.label'
  | 'ocr.onlyPdf'
  | 'ocr.provider.wechat'
  | 'ocr.provider.baidu'
  | 'settings.toggle'
  | 'settings.root'
  | 'settings.wechatUrl'
  | 'settings.baiduKey'
  | 'settings.baiduSecret'
  | 'settings.calibre.found'
  | 'settings.calibre.missing'
  | 'settings.calibre.pick'
  | 'settings.calibre.install'
  | 'settings.save'
  | 'queue.empty'
  | 'status.pending'
  | 'status.running'
  | 'status.done'
  | 'status.failed'
  | 'status.cancelled'
  | 'action.openInEditor'
  | 'action.cancel'
  | 'action.clear'
  | 'log.toggle'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Ebook Import',
  'drop.hint': 'Drag epub / pdf / docx files here',
  'drop.pick': 'Add files…',
  'ocr.label': 'OCR (scanned PDF)',
  'ocr.onlyPdf': 'OCR only applies to PDF',
  'ocr.provider.wechat': 'WeChat OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': 'Settings',
  'settings.root': 'Ebooks root',
  'settings.wechatUrl': 'WeChat OCR URL',
  'settings.baiduKey': 'Baidu API key',
  'settings.baiduSecret': 'Baidu secret key',
  'settings.calibre.found': 'Calibre found: {path} ({version})',
  'settings.calibre.missing': 'Calibre not found',
  'settings.calibre.pick': 'Choose…',
  'settings.calibre.install': 'Install Calibre',
  'settings.save': 'Save',
  'queue.empty': 'No imports yet — drop a file to start.',
  'status.pending': 'Pending',
  'status.running': 'Running',
  'status.done': 'Done',
  'status.failed': 'Failed',
  'status.cancelled': 'Cancelled',
  'action.openInEditor': 'Open in editor',
  'action.cancel': 'Cancel',
  'action.clear': 'Clear finished',
  'log.toggle': 'Log',
}

const zh: Catalog = {
  title: '导入电子书',
  'drop.hint': '将 epub / pdf / docx 文件拖到这里',
  'drop.pick': '添加文件…',
  'ocr.label': 'OCR(扫描版 PDF)',
  'ocr.onlyPdf': 'OCR 仅对 PDF 生效',
  'ocr.provider.wechat': '微信OCR',
  'ocr.provider.baidu': '百度 Unlimited-OCR',
  'settings.toggle': '设置',
  'settings.root': '电子书根目录',
  'settings.wechatUrl': '微信 OCR 地址',
  'settings.baiduKey': '百度 API Key',
  'settings.baiduSecret': '百度 Secret Key',
  'settings.calibre.found': '已找到 Calibre：{path}（{version}）',
  'settings.calibre.missing': '未找到 Calibre',
  'settings.calibre.pick': '选择…',
  'settings.calibre.install': '安装 Calibre',
  'settings.save': '保存',
  'queue.empty': '暂无导入任务——拖入文件开始。',
  'status.pending': '等待中',
  'status.running': '进行中',
  'status.done': '已完成',
  'status.failed': '失败',
  'status.cancelled': '已取消',
  'action.openInEditor': '在编辑器打开',
  'action.cancel': '取消',
  'action.clear': '清除已完成',
  'log.toggle': '日志',
}

const ja: Catalog = {
  title: '電子書籍を取り込む',
  'drop.hint': 'epub・pdf・docx ファイルをここにドロップ',
  'drop.pick': 'ファイルを追加…',
  'ocr.label': 'OCR(スキャン PDF)',
  'ocr.onlyPdf': 'OCR は PDF にのみ適用されます',
  'ocr.provider.wechat': 'WeChat OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': '設定',
  'settings.root': '電子書籍のルート',
  'settings.wechatUrl': 'WeChat OCR の URL',
  'settings.baiduKey': 'Baidu API キー',
  'settings.baiduSecret': 'Baidu シークレットキー',
  'settings.calibre.found': 'Calibre が見つかりました：{path}（{version}）',
  'settings.calibre.missing': 'Calibre が見つかりません',
  'settings.calibre.pick': '選択…',
  'settings.calibre.install': 'Calibre をインストール',
  'settings.save': '保存',
  'queue.empty': 'インポートはまだありません。ファイルをドロップして開始してください。',
  'status.pending': '待機中',
  'status.running': '実行中',
  'status.done': '完了',
  'status.failed': '失敗',
  'status.cancelled': 'キャンセル済み',
  'action.openInEditor': 'エディタで開く',
  'action.cancel': 'キャンセル',
  'action.clear': '完了分を消去',
  'log.toggle': 'ログ',
}

const de: Catalog = {
  title: 'E-Books importieren',
  'drop.hint': 'epub-, pdf- oder docx-Dateien hierher ziehen',
  'drop.pick': 'Dateien hinzufügen…',
  'ocr.label': 'OCR (gescanntes PDF)',
  'ocr.onlyPdf': 'OCR gilt nur für PDF',
  'ocr.provider.wechat': 'WeChat-OCR',
  'ocr.provider.baidu': 'Baidu Unlimited-OCR',
  'settings.toggle': 'Einstellungen',
  'settings.root': 'E-Book-Stammverzeichnis',
  'settings.wechatUrl': 'WeChat-OCR-URL',
  'settings.baiduKey': 'Baidu-API-Schlüssel',
  'settings.baiduSecret': 'Baidu-Geheimschlüssel',
  'settings.calibre.found': 'Calibre gefunden: {path} ({version})',
  'settings.calibre.missing': 'Calibre nicht gefunden',
  'settings.calibre.pick': 'Auswählen…',
  'settings.calibre.install': 'Calibre installieren',
  'settings.save': 'Speichern',
  'queue.empty': 'Noch keine Importe — Datei hierher ziehen, um zu beginnen.',
  'status.pending': 'Ausstehend',
  'status.running': 'Läuft',
  'status.done': 'Fertig',
  'status.failed': 'Fehlgeschlagen',
  'status.cancelled': 'Abgebrochen',
  'action.openInEditor': 'Im Editor öffnen',
  'action.cancel': 'Abbrechen',
  'action.clear': 'Fertige entfernen',
  'log.toggle': 'Protokoll',
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

/**
 * Translates `key` for the active locale, filling `{name}` placeholders from
 * `params`. Falls back to the English catalog for a missing key, then to the
 * raw key.
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
