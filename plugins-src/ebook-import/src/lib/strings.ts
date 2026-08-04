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
  | 'action.start'
  | 'action.running'
  | 'action.aiRead'
  | 'action.viewSummary'
  | 'ai.queued'
  | 'ai.running'
  | 'ai.failed'
  | 'log.toggle'
  | 'stage.convert'
  | 'stage.extract'
  | 'stage.markdown'
  | 'stage.ocr'
  | 'stage.finalize'
  | 'dialog.ebooksFilter'
  | 'dialog.pickBooks'
  | 'dialog.pickCalibre'
  | 'err.noVault'
  | 'err.calibreMissing'
  | 'err.calibreTimeout'
  | 'err.calibreFailed'
  | 'err.badRoot'
  | 'err.noTitle'
  | 'err.ocrOnlyPdf'
  | 'err.ocrEmpty'
  | 'err.ocrUnreachable'
  | 'err.baiduFailed'
  | 'err.baiduCredentials'
  | 'err.baiduAuth'
  | 'settings.baiduHint'
  | 'ocr.baiduCost'
  | 'err.unsupportedType'
  | 'log.ocrStart'
  | 'log.converting'
  | 'log.pageFailed'
  | 'log.failedPages'
  | 'log.baiduToken'
  | 'log.baiduSubmit'
  | 'log.baiduDownload'
  | 'log.baiduStatus'
  | 'log.baiduStatus.pending'
  | 'log.baiduStatus.running'
  | 'log.baiduStatus.success'
  | 'log.baiduStatus.failed'
  | 'settings.ocrProvider'

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
  'settings.baiduHint':
    'Baidu Cloud console → AI → OCR / Document parsing → Applications → create one; its API Key and Secret Key go below. Billed per page, roughly ¥0.1 (CNY) each.',
  'ocr.baiduCost': 'Paid service — roughly ¥0.1 (CNY) per page',
  'settings.baiduSecret': 'Baidu secret key',
  'settings.calibre.found': 'Calibre found: {path} ({version})',
  'settings.calibre.missing': 'Calibre not found',
  'settings.calibre.pick': 'Choose…',
  'settings.calibre.install': 'Install Calibre',
  'settings.save': 'Save',
  'queue.empty': 'Nothing queued yet — add files, choose whether to OCR, then press Start.',
  'status.pending': 'Pending',
  'status.running': 'Running',
  'status.done': 'Done',
  'status.failed': 'Failed',
  'status.cancelled': 'Cancelled',
  'action.openInEditor': 'Open in editor',
  'action.cancel': 'Cancel',
  'action.clear': 'Clear finished',
  'action.start': 'Start import',
  'action.running': 'Importing…',
  'action.aiRead': 'AI read first',
  'action.viewSummary': 'View digest',
  'ai.queued': 'Waiting for AI…',
  'ai.running': 'AI reading… {elapsed}',
  'ai.failed': 'AI reading failed',
  'log.toggle': 'Log',
  'stage.convert': 'Converting',
  'stage.extract': 'Extracting',
  'stage.markdown': 'Converting to Markdown',
  'stage.ocr': 'Running OCR',
  'stage.finalize': 'Finalizing',
  'dialog.ebooksFilter': 'Ebooks',
  'dialog.pickBooks': 'Choose ebook files',
  'dialog.pickCalibre': 'Choose the ebook-convert binary',
  'err.noVault': 'No vault configured — open or create a vault first.',
  'err.calibreMissing': 'Calibre not found — install it or pick its path in Settings.',
  'err.calibreTimeout': 'Calibre conversion timed out — try a smaller file or check Settings.',
  'err.calibreFailed': 'Calibre conversion failed — see the log for details.',
  'err.badRoot': 'The ebooks root must be a path inside the vault — check Settings.',
  'err.noTitle': "Couldn't determine a title for this book.",
  'err.ocrOnlyPdf': 'OCR only works on PDF files.',
  'err.ocrEmpty': 'OCR produced no text — the scan may be blank or unreadable.',
  'err.ocrUnreachable': "Couldn't reach the OCR service — check your network or Settings.",
  'err.baiduFailed': 'Baidu OCR failed — see the log for details.',
  'err.baiduCredentials':
    "Baidu didn't recognize the credentials. Use an application's API Key and Secret Key from the OCR console — not an Access Key pair — and check the two fields aren't swapped.",
  'err.baiduAuth': 'Baidu sign-in failed — check the API key and secret key in Settings.',
  'err.unsupportedType': 'Unsupported file type — only epub, pdf, and docx are supported.',
  'log.ocrStart': 'OCR: {file}',
  'log.converting': 'Converting {file} with Calibre…',
  'log.pageFailed': 'Page {page} failed: {reason}',
  'log.failedPages': 'Pages that failed OCR: {pages}',
  'log.baiduToken': 'Baidu OCR: requesting an access token…',
  'log.baiduSubmit': 'Baidu OCR: uploading the document…',
  'log.baiduDownload': 'Baidu OCR: downloading the markdown…',
  'log.baiduStatus': 'Baidu OCR: {status}',
  'log.baiduStatus.pending': 'queued',
  'log.baiduStatus.running': 'processing',
  'log.baiduStatus.success': 'done',
  'log.baiduStatus.failed': 'failed',
  'settings.ocrProvider': 'OCR service',
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
  'settings.baiduHint':
    '在百度智能云控制台 → 人工智能 → 文字识别/文档解析 → 应用列表 → 创建应用,把该应用的 API Key 和 Secret Key 填在下面。按页计费,约 ¥0.1/页。',
  'ocr.baiduCost': '按页付费,约 ¥0.1/页',
  'settings.baiduSecret': '百度 Secret Key',
  'settings.calibre.found': '已找到 Calibre：{path}（{version}）',
  'settings.calibre.missing': '未找到 Calibre',
  'settings.calibre.pick': '选择…',
  'settings.calibre.install': '安装 Calibre',
  'settings.save': '保存',
  'queue.empty': '任务列表为空——先添加文件、选好是否 OCR,再点「开始导入」。',
  'status.pending': '等待中',
  'status.running': '进行中',
  'status.done': '已完成',
  'status.failed': '失败',
  'status.cancelled': '已取消',
  'action.openInEditor': '在编辑器打开',
  'action.cancel': '取消',
  'action.clear': '清除已完成',
  'action.start': '开始导入',
  'action.running': '导入中…',
  'action.aiRead': 'AI 先读',
  'action.viewSummary': '查看摘要',
  'ai.queued': '排队等待 AI 阅读…',
  'ai.running': 'AI 阅读中… {elapsed}',
  'ai.failed': 'AI 阅读失败',
  'log.toggle': '日志',
  'stage.convert': '转换中',
  'stage.extract': '解包中',
  'stage.markdown': '生成 Markdown 中',
  'stage.ocr': 'OCR 识别中',
  'stage.finalize': '整理中',
  'dialog.ebooksFilter': '电子书',
  'dialog.pickBooks': '选择电子书文件',
  'dialog.pickCalibre': '选择 ebook-convert 可执行文件',
  'err.noVault': '未配置 vault——请先打开或创建一个 vault。',
  'err.calibreMissing': '未找到 Calibre——请安装,或在设置中指定路径。',
  'err.calibreTimeout': 'Calibre 转换超时——请尝试更小的文件,或检查设置。',
  'err.calibreFailed': 'Calibre 转换失败——详情见日志。',
  'err.badRoot': '电子书根目录必须是 vault 内的路径——请检查设置。',
  'err.noTitle': '无法确定这本书的标题。',
  'err.ocrOnlyPdf': 'OCR 仅支持 PDF 文件。',
  'err.ocrEmpty': 'OCR 未识别出任何文字——扫描件可能是空白或无法识别。',
  'err.ocrUnreachable': '无法连接 OCR 服务——请检查网络或设置。',
  'err.baiduFailed': '百度 OCR 失败——详情见日志。',
  'err.baiduCredentials':
    '百度不认这组凭据。请填 OCR 控制台中「应用」的 API Key 和 Secret Key(不是「安全认证」里的 Access Key),并确认两栏没有填反。',
  'err.baiduAuth': '百度鉴权失败——请检查设置里的 API Key 和 Secret Key。',
  'err.unsupportedType': '不支持的文件类型——仅支持 epub、pdf、docx。',
  'log.ocrStart': 'OCR:{file}',
  'log.converting': '正在用 Calibre 转换 {file}…',
  'log.pageFailed': '第 {page} 页失败:{reason}',
  'log.failedPages': 'OCR 失败的页码:{pages}',
  'log.baiduToken': '百度 OCR:正在获取访问令牌…',
  'log.baiduSubmit': '百度 OCR:正在上传文档…',
  'log.baiduDownload': '百度 OCR:正在下载识别结果…',
  'log.baiduStatus': '百度 OCR:{status}',
  'log.baiduStatus.pending': '排队中',
  'log.baiduStatus.running': '识别中',
  'log.baiduStatus.success': '已完成',
  'log.baiduStatus.failed': '失败',
  'settings.ocrProvider': 'OCR 服务',
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
  'settings.baiduHint':
    'Baidu Cloud コンソール → 人工知能 → 文字認識/文書解析 → アプリケーション一覧 → アプリを作成し、その API Key と Secret Key を下に入力します。ページ単位の課金で、1 ページ約 0.1 元です。',
  'ocr.baiduCost': '有料サービス — 1 ページ約 0.1 元',
  'settings.baiduSecret': 'Baidu シークレットキー',
  'settings.calibre.found': 'Calibre が見つかりました：{path}（{version}）',
  'settings.calibre.missing': 'Calibre が見つかりません',
  'settings.calibre.pick': '選択…',
  'settings.calibre.install': 'Calibre をインストール',
  'settings.save': '保存',
  'queue.empty': 'まだ何もありません。ファイルを追加し、OCR の有無を選んでから「開始」を押してください。',
  'status.pending': '待機中',
  'status.running': '実行中',
  'status.done': '完了',
  'status.failed': '失敗',
  'status.cancelled': 'キャンセル済み',
  'action.openInEditor': 'エディタで開く',
  'action.cancel': 'キャンセル',
  'action.clear': '完了分を消去',
  'action.start': '取り込みを開始',
  'action.running': '取り込み中…',
  'action.aiRead': 'AI に先に読ませる',
  'action.viewSummary': '要約を見る',
  'ai.queued': 'AI リーディング待機中…',
  'ai.running': 'AI リーディング中… {elapsed}',
  'ai.failed': 'AI リーディングに失敗しました',
  'log.toggle': 'ログ',
  'stage.convert': '変換中',
  'stage.extract': '展開中',
  'stage.markdown': 'Markdown に変換中',
  'stage.ocr': 'OCR 実行中',
  'stage.finalize': '仕上げ中',
  'dialog.ebooksFilter': '電子書籍',
  'dialog.pickBooks': '電子書籍ファイルを選択',
  'dialog.pickCalibre': 'ebook-convert 実行ファイルを選択',
  'err.noVault': 'vault が未設定です。まず vault を開くか作成してください。',
  'err.calibreMissing': 'Calibre が見つかりません。インストールするか、設定でパスを指定してください。',
  'err.calibreTimeout': 'Calibre の変換がタイムアウトしました。ファイルサイズを小さくするか、設定を確認してください。',
  'err.calibreFailed': 'Calibre の変換に失敗しました。詳細はログを確認してください。',
  'err.badRoot': '電子書籍のルートは vault 内のパスである必要があります。設定を確認してください。',
  'err.noTitle': 'この書籍のタイトルを特定できませんでした。',
  'err.ocrOnlyPdf': 'OCR は PDF ファイルにのみ対応しています。',
  'err.ocrEmpty': 'OCR でテキストを認識できませんでした。スキャンが空白か読み取れない可能性があります。',
  'err.ocrUnreachable': 'OCR サービスに接続できません。ネットワークまたは設定を確認してください。',
  'err.baiduFailed': 'Baidu OCR が失敗しました。詳細はログを確認してください。',
  'err.baiduCredentials':
    'Baidu が認証情報を受け付けませんでした。OCR コンソールの「アプリケーション」の API Key と Secret Key(「セキュリティ認証」の Access Key ではありません)を入力し、2 つの欄が入れ替わっていないか確認してください。',
  'err.baiduAuth': 'Baidu の認証に失敗しました。設定の API キーとシークレットキーを確認してください。',
  'err.unsupportedType': 'サポートされていないファイル形式です。epub、pdf、docx のみ対応しています。',
  'log.ocrStart': 'OCR:{file}',
  'log.converting': 'Calibre で {file} を変換中…',
  'log.pageFailed': '{page} ページ目が失敗:{reason}',
  'log.failedPages': 'OCR に失敗したページ:{pages}',
  'log.baiduToken': 'Baidu OCR:アクセストークンを取得中…',
  'log.baiduSubmit': 'Baidu OCR:ドキュメントをアップロード中…',
  'log.baiduDownload': 'Baidu OCR:結果をダウンロード中…',
  'log.baiduStatus': 'Baidu OCR:{status}',
  'log.baiduStatus.pending': '待機中',
  'log.baiduStatus.running': '処理中',
  'log.baiduStatus.success': '完了',
  'log.baiduStatus.failed': '失敗',
  'settings.ocrProvider': 'OCR サービス',
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
  'settings.baiduHint':
    'Baidu-Cloud-Konsole → KI → Texterkennung/Dokumentanalyse → Anwendungen → eine anlegen; deren API Key und Secret Key gehören hier hinein. Abrechnung pro Seite, etwa ¥0,1 (CNY) je Seite.',
  'ocr.baiduCost': 'Kostenpflichtig — etwa ¥0,1 (CNY) pro Seite',
  'settings.baiduSecret': 'Baidu-Geheimschlüssel',
  'settings.calibre.found': 'Calibre gefunden: {path} ({version})',
  'settings.calibre.missing': 'Calibre nicht gefunden',
  'settings.calibre.pick': 'Auswählen…',
  'settings.calibre.install': 'Calibre installieren',
  'settings.save': 'Speichern',
  'queue.empty': 'Noch nichts in der Liste — Dateien hinzufügen, OCR wählen, dann auf Start drücken.',
  'status.pending': 'Ausstehend',
  'status.running': 'Läuft',
  'status.done': 'Fertig',
  'status.failed': 'Fehlgeschlagen',
  'status.cancelled': 'Abgebrochen',
  'action.openInEditor': 'Im Editor öffnen',
  'action.cancel': 'Abbrechen',
  'action.clear': 'Fertige entfernen',
  'action.start': 'Import starten',
  'action.running': 'Importiert…',
  'action.aiRead': 'Zuerst KI lesen lassen',
  'action.viewSummary': 'Zusammenfassung öffnen',
  'ai.queued': 'Wartet auf KI…',
  'ai.running': 'KI liest… {elapsed}',
  'ai.failed': 'KI-Lektüre fehlgeschlagen',
  'log.toggle': 'Protokoll',
  'stage.convert': 'Wird konvertiert',
  'stage.extract': 'Wird entpackt',
  'stage.markdown': 'Wird in Markdown umgewandelt',
  'stage.ocr': 'OCR läuft',
  'stage.finalize': 'Wird abgeschlossen',
  'dialog.ebooksFilter': 'E-Books',
  'dialog.pickBooks': 'E-Book-Dateien auswählen',
  'dialog.pickCalibre': 'Wählen Sie die ebook-convert-Binärdatei',
  'err.noVault': 'Kein Vault konfiguriert — bitte zuerst einen Vault öffnen oder erstellen.',
  'err.calibreMissing': 'Calibre nicht gefunden — installieren oder den Pfad in den Einstellungen angeben.',
  'err.calibreTimeout': 'Zeitüberschreitung bei der Calibre-Konvertierung — kleinere Datei versuchen oder Einstellungen prüfen.',
  'err.calibreFailed': 'Calibre-Konvertierung fehlgeschlagen — Details im Protokoll.',
  'err.badRoot': 'Das E-Book-Stammverzeichnis muss ein Pfad innerhalb des Vaults sein — Einstellungen prüfen.',
  'err.noTitle': 'Für dieses Buch konnte kein Titel ermittelt werden.',
  'err.ocrOnlyPdf': 'OCR funktioniert nur mit PDF-Dateien.',
  'err.ocrEmpty': 'OCR hat keinen Text erkannt — der Scan ist möglicherweise leer oder unlesbar.',
  'err.ocrUnreachable': 'OCR-Dienst nicht erreichbar — Netzwerk oder Einstellungen prüfen.',
  'err.baiduFailed': 'Baidu-OCR fehlgeschlagen — Details im Protokoll.',
  'err.baiduCredentials':
    'Baidu hat die Zugangsdaten nicht erkannt. Verwende API Key und Secret Key einer Anwendung aus der OCR-Konsole — kein Access-Key-Paar — und prüfe, ob die beiden Felder vertauscht sind.',
  'err.baiduAuth': 'Baidu-Anmeldung fehlgeschlagen — API-Key und Secret Key in den Einstellungen prüfen.',
  'err.unsupportedType': 'Nicht unterstützter Dateityp — nur epub, pdf und docx werden unterstützt.',
  'log.ocrStart': 'OCR läuft: {file}',
  'log.converting': 'Konvertiere {file} mit Calibre…',
  'log.pageFailed': 'Seite {page} fehlgeschlagen: {reason}',
  'log.failedPages': 'Seiten mit OCR-Fehler: {pages}',
  'log.baiduToken': 'Baidu OCR: Zugriffstoken wird angefordert…',
  'log.baiduSubmit': 'Baidu OCR: Dokument wird hochgeladen…',
  'log.baiduDownload': 'Baidu OCR: Markdown wird heruntergeladen…',
  'log.baiduStatus': 'Baidu-OCR: {status}',
  'log.baiduStatus.pending': 'in der Warteschlange',
  'log.baiduStatus.running': 'wird verarbeitet',
  'log.baiduStatus.success': 'fertig',
  'log.baiduStatus.failed': 'fehlgeschlagen',
  'settings.ocrProvider': 'OCR-Dienst',
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
