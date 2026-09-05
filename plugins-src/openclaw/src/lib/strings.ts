// src/lib/strings.ts — self-contained i18n for the openclaw-chat plugin.
//
// The 13 keys the chat UI uses are copied verbatim (2026-07) from the host
// catalogs src/lib/i18n/{en,zh,ja,de}.ts. Keys keep their host namespace
// (`chat.*` / `common.cancel`) so the ported component call sites are unchanged.
// Language is chosen by `notemd.locale` at startup; English is the fallback.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'chat.session'
  | 'chat.messages'
  | 'chat.sending'
  | 'chat.sendFailed'
  | 'chat.uploadFailed'
  | 'chat.sendShortcut'
  | 'common.cancel'
  | 'chat.connectTitle'
  | 'chat.enterPairingCode'
  | 'chat.pairingCode'
  | 'chat.deviceNameOptional'
  | 'chat.connecting'
  | 'chat.pair'
  | 'chat.addDevice'
  | 'chat.retry'
  | 'chat.generatingCode'
  | 'chat.expiresIn'
  | 'chat.typeToOpenClaw'
  | 'chat.send'
  | 'chat.detecting'
  | 'chat.initError'
  | 'chat.newSession'
  | 'chat.noMessages'
  | 'chat.newDeviceWantsToConnect'
  | 'chat.allow'
  | 'chat.reject'
  | 'chat.attach'
  | 'chat.status.idle'
  | 'chat.status.connecting'
  | 'chat.status.connected'
  | 'chat.status.disconnected'
  | 'chat.role.user'
  | 'chat.role.agent'
  | 'chat.role.system'
  | 'err.noAccessToken'
  | 'err.noRelayUrl'
  | 'err.pairingFailed'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  'chat.session': "Conversation",
  'chat.messages': "Messages",
  'chat.sending': "Sending…",
  'chat.sendFailed': "Message not sent. Your draft is still here",
  'chat.uploadFailed': "Attachment upload failed. Select the file to retry",
  'chat.sendShortcut': "⌘/Ctrl + Enter to send · Enter for a new line",
  'common.cancel': 'Cancel',
  'chat.connectTitle': 'Connect to your OpenClaw',
  'chat.enterPairingCode': "Enter the pairing code shown on your host machine's note.md settings.",
  'chat.pairingCode': 'Pairing code',
  'chat.deviceNameOptional': 'Device name (optional)',
  'chat.connecting': 'Connecting…',
  'chat.pair': 'Pair',
  'chat.addDevice': 'Add a new device',
  'chat.retry': 'Retry',
  'chat.generatingCode': 'Generating pairing code…',
  'chat.expiresIn': 'Expires in {time}',
  'chat.typeToOpenClaw': 'Type to OpenClaw…',
  'chat.send': 'Send',
  'chat.detecting': 'Detecting…',
  'chat.initError': 'init error',
  'chat.newSession': '+ New',
  'chat.noMessages': 'No messages yet. Say hi.',
  'chat.newDeviceWantsToConnect': 'New device wants to connect:',
  'chat.allow': 'Allow',
  'chat.reject': 'Reject',
  'chat.attach': 'Attach a file',
  'chat.status.idle': 'Idle',
  'chat.status.connecting': 'Connecting…',
  'chat.status.connected': 'Connected',
  'chat.status.disconnected': 'Disconnected',
  'chat.role.user': 'You',
  'chat.role.agent': 'OpenClaw',
  'chat.role.system': 'System',
  'err.noAccessToken': "OpenClaw's access token isn't set up yet — open OpenClaw once on the host to generate it.",
  'err.noRelayUrl': 'No relay URL configured — set one up in OpenClaw settings.',
  'err.pairingFailed': 'Pairing failed (status {status}).',
}

const zh: Catalog = {
  'chat.session': "当前会话",
  'chat.messages': "会话消息",
  'chat.sending': "正在发送…",
  'chat.sendFailed': "消息未发送，输入内容已保留",
  'chat.uploadFailed': "附件上传失败，请重新选择文件重试",
  'chat.sendShortcut': "⌘/Ctrl + Enter 发送 · Enter 换行",
  'common.cancel': '取消',
  'chat.connectTitle': '连接到你的 OpenClaw',
  'chat.enterPairingCode': '输入你主机上 note.md 设置中显示的配对码。',
  'chat.pairingCode': '配对码',
  'chat.deviceNameOptional': '设备名称（可选）',
  'chat.connecting': '连接中…',
  'chat.pair': '配对',
  'chat.addDevice': '添加新设备',
  'chat.retry': '重试',
  'chat.generatingCode': '正在生成配对码…',
  'chat.expiresIn': '{time} 后过期',
  'chat.typeToOpenClaw': '输入发送给 OpenClaw…',
  'chat.send': '发送',
  'chat.detecting': '检测中…',
  'chat.initError': '初始化错误',
  'chat.newSession': '+ 新建',
  'chat.noMessages': '还没有消息，打个招呼吧。',
  'chat.newDeviceWantsToConnect': '有新设备请求连接：',
  'chat.allow': '允许',
  'chat.reject': '拒绝',
  'chat.attach': '添加附件',
  'chat.status.idle': '空闲',
  'chat.status.connecting': '连接中…',
  'chat.status.connected': '已连接',
  'chat.status.disconnected': '已断开',
  'chat.role.user': '我',
  'chat.role.agent': 'OpenClaw',
  'chat.role.system': '系统',
  'err.noAccessToken': '尚未配置 OpenClaw 访问令牌——请先在主机上打开一次 OpenClaw,让它自动生成。',
  'err.noRelayUrl': '未配置中转(relay)地址——请在 OpenClaw 设置中配置。',
  'err.pairingFailed': '配对失败(状态 {status})。',
}

const ja: Catalog = {
  'chat.session': "会話",
  'chat.messages': "メッセージ",
  'chat.sending': "送信中…",
  'chat.sendFailed': "送信できませんでした。入力内容は保持されています",
  'chat.uploadFailed': "添付に失敗しました。ファイルを選び直して再試行してください",
  'chat.sendShortcut': "⌘/Ctrl + Enter で送信 · Enter で改行",
  'common.cancel': 'キャンセル',
  'chat.connectTitle': 'OpenClaw に接続',
  'chat.enterPairingCode': 'ホストマシンの note.md 設定に表示されるペアリングコードを入力してください。',
  'chat.pairingCode': 'ペアリングコード',
  'chat.deviceNameOptional': 'デバイス名（任意）',
  'chat.connecting': '接続中…',
  'chat.pair': 'ペアリング',
  'chat.addDevice': '新しいデバイスを追加',
  'chat.retry': '再試行',
  'chat.generatingCode': 'ペアリングコードを生成中…',
  'chat.expiresIn': '{time} で期限切れ',
  'chat.typeToOpenClaw': 'OpenClaw に入力…',
  'chat.send': '送信',
  'chat.detecting': '検出中…',
  'chat.initError': '初期化エラー',
  'chat.newSession': '+ 新規',
  'chat.noMessages': 'まだメッセージがありません。挨拶してみましょう。',
  'chat.newDeviceWantsToConnect': '新しいデバイスが接続を求めています：',
  'chat.allow': '許可',
  'chat.reject': '拒否',
  'chat.attach': 'ファイルを添付',
  'chat.status.idle': '待機中',
  'chat.status.connecting': '接続中…',
  'chat.status.connected': '接続済み',
  'chat.status.disconnected': '切断済み',
  'chat.role.user': '自分',
  'chat.role.agent': 'OpenClaw',
  'chat.role.system': 'システム',
  'err.noAccessToken': 'OpenClaw のアクセストークンがまだ設定されていません。ホスト側で一度 OpenClaw を開いて自動生成してください。',
  'err.noRelayUrl': 'リレー URL が未設定です。OpenClaw の設定で指定してください。',
  'err.pairingFailed': 'ペアリングに失敗しました(ステータス {status})。',
}

const de: Catalog = {
  'chat.session': "Unterhaltung",
  'chat.messages': "Nachrichten",
  'chat.sending': "Wird gesendet…",
  'chat.sendFailed': "Nachricht nicht gesendet. Der Entwurf bleibt erhalten",
  'chat.uploadFailed': "Anhang konnte nicht hochgeladen werden. Datei erneut auswählen",
  'chat.sendShortcut': "⌘/Strg + Eingabe zum Senden · Eingabe für eine neue Zeile",
  'common.cancel': 'Abbrechen',
  'chat.connectTitle': 'Mit Ihrem OpenClaw verbinden',
  'chat.enterPairingCode': 'Geben Sie den Kopplungscode ein, der in den note.md-Einstellungen Ihres Host-Computers angezeigt wird.',
  'chat.pairingCode': 'Kopplungscode',
  'chat.deviceNameOptional': 'Gerätename (optional)',
  'chat.connecting': 'Verbindet…',
  'chat.pair': 'Koppeln',
  'chat.addDevice': 'Neues Gerät hinzufügen',
  'chat.retry': 'Erneut versuchen',
  'chat.generatingCode': 'Kopplungscode wird generiert…',
  'chat.expiresIn': 'Läuft ab in {time}',
  'chat.typeToOpenClaw': 'An OpenClaw schreiben…',
  'chat.send': 'Senden',
  'chat.detecting': 'Wird erkannt…',
  'chat.initError': 'Initialisierungsfehler',
  'chat.newSession': '+ Neu',
  'chat.noMessages': 'Noch keine Nachrichten. Sag Hallo.',
  'chat.newDeviceWantsToConnect': 'Neues Gerät möchte sich verbinden:',
  'chat.allow': 'Zulassen',
  'chat.reject': 'Ablehnen',
  'chat.attach': 'Datei anhängen',
  'chat.status.idle': 'Inaktiv',
  'chat.status.connecting': 'Verbindet…',
  'chat.status.connected': 'Verbunden',
  'chat.status.disconnected': 'Getrennt',
  'chat.role.user': 'Du',
  'chat.role.agent': 'OpenClaw',
  'chat.role.system': 'System',
  'err.noAccessToken':
    'Der OpenClaw-Zugriffstoken ist noch nicht eingerichtet — OpenClaw einmal auf dem Host öffnen, damit er erzeugt wird.',
  'err.noRelayUrl': 'Keine Relay-URL konfiguriert — in den OpenClaw-Einstellungen einrichten.',
  'err.pairingFailed': 'Kopplung fehlgeschlagen (Status {status}).',
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
 * Translate `key`, filling `{name}` placeholders from `params`. Falls back to
 * the English catalog for a missing key, then to the raw key. Mirrors the
 * host's `t()` (src/lib/i18n/store.svelte.ts).
 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const catalog = registry[active] ?? en
  let s: string = catalog[key] ?? en[key] ?? key
  if (params) {
    s = s.replace(/\{(\w+)\}/g, (m, name) => (name in params ? String(params[name]) : m))
  }
  return s
}

// Exported for tests only (catalog completeness / placeholder parity checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
