// 插件自带 i18n。隔离 webview 用不了主程序的 t();结构照抄
// plugins-src/openclaw/src/lib/strings.ts。
export type MessageKey =
  | 'title' | 'surfaces.section' | 'surfaces.main' | 'surfaces.hint'
  | 'effects.section'
  | 'explosion.enable' | 'explosion.preset'
  | 'preset.particle' | 'preset.lightning' | 'preset.coin' | 'preset.confetti'
  | 'shake.enable' | 'shake.intensity'
  | 'shake.level.light' | 'shake.level.medium' | 'shake.level.heavy'
  | 'combo.enable' | 'combo.timeout' | 'combo.showExclamation' | 'combo.precisionInput'
  | 'combo.precisionInput.hint'
  | 'combo.timeout.short' | 'combo.timeout.medium' | 'combo.timeout.long'
  | 'demo.section' | 'demo.hint' | 'demo.sample' | 'demo.unavailable'
  | 'saveFailed'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Power Mode',
  'surfaces.section': 'Where it applies',
  'surfaces.main': 'Main editor window',
  'surfaces.hint': 'Plugin windows that embed the host editor appear here automatically.',
  'effects.section': 'Effects',
  'explosion.enable': 'Cursor explosions',
  'explosion.preset': 'Preset',
  'preset.particle': 'Particle',
  'preset.lightning': 'Lightning',
  'preset.coin': 'Coin',
  'preset.confetti': 'Confetti',
  'shake.enable': 'Screen shake',
  'shake.intensity': 'Intensity',
  'shake.level.light': 'Light',
  'shake.level.medium': 'Medium',
  'shake.level.heavy': 'Heavy',
  'combo.enable': 'Combo meter',
  'combo.timeout': 'Timeout',
  'combo.timeout.short': 'Short',
  'combo.timeout.medium': 'Medium',
  'combo.timeout.long': 'Long',
  'combo.showExclamation': 'Exclamations',
  'combo.precisionInput': 'Precision input',
  'combo.precisionInput.hint': 'Only count edits that do not shorten the document.',
  'demo.section': 'Try it',
  'demo.hint': 'Type here — the settings above apply live, regardless of the switches.',
  'demo.sample': 'Type something and watch the sparks fly.',
  'demo.unavailable': 'The live preview needs a newer version of note.md.',
  saveFailed: 'Could not save settings',
}

const zh: Catalog = {
  title: '狂暴模式',
  'surfaces.section': '生效范围',
  'surfaces.main': '主编辑窗口',
  'surfaces.hint': '内嵌宿主编辑器的插件窗口会自动出现在这里。',
  'effects.section': '特效',
  'explosion.enable': '光标爆炸',
  'explosion.preset': '预设',
  'preset.particle': '粒子',
  'preset.lightning': '闪电',
  'preset.coin': '金币',
  'preset.confetti': '彩纸',
  'shake.enable': '屏幕震动',
  'shake.intensity': '强度',
  'shake.level.light': '轻度',
  'shake.level.medium': '中度',
  'shake.level.heavy': '重度',
  'combo.enable': '连击计数',
  'combo.timeout': '超时',
  'combo.timeout.short': '短',
  'combo.timeout.medium': '中',
  'combo.timeout.long': '长',
  'combo.showExclamation': '感叹词',
  'combo.precisionInput': '精确输入',
  'combo.precisionInput.hint': '只统计没让文档变短的编辑。',
  'demo.section': '试试看',
  'demo.hint': '在这里敲字 —— 上面的设置立刻生效,不受开关影响。',
  'demo.sample': '敲点什么,看看火花。',
  'demo.unavailable': '实操区需要更新版本的 note.md。',
  saveFailed: '设置保存失败',
}

const ja: Catalog = {
  title: 'パワーモード',
  'surfaces.section': '適用範囲',
  'surfaces.main': 'メインエディタウィンドウ',
  'surfaces.hint': 'ホストエディタを埋め込むプラグインウィンドウは自動的にここに表示されます。',
  'effects.section': 'エフェクト',
  'explosion.enable': 'カーソル爆発',
  'explosion.preset': 'プリセット',
  'preset.particle': 'パーティクル',
  'preset.lightning': 'ライトニング',
  'preset.coin': 'コイン',
  'preset.confetti': '紙吹雪',
  'shake.enable': '画面シェイク',
  'shake.intensity': '強さ',
  'shake.level.light': '弱',
  'shake.level.medium': '中',
  'shake.level.heavy': '強',
  'combo.enable': 'コンボカウンター',
  'combo.timeout': 'タイムアウト',
  'combo.timeout.short': '短',
  'combo.timeout.medium': '中',
  'combo.timeout.long': '長',
  'combo.showExclamation': '感嘆詞',
  'combo.precisionInput': '精密入力',
  'combo.precisionInput.hint': '文書が短くならない編集だけを数えます。',
  'demo.section': '試す',
  'demo.hint': 'ここで入力してください。上の設定がスイッチに関係なく即座に反映されます。',
  'demo.sample': '何か入力して火花を見てみましょう。',
  'demo.unavailable': 'ライブプレビューには新しいバージョンの note.md が必要です。',
  saveFailed: '設定を保存できませんでした',
}

const de: Catalog = {
  title: 'Power-Modus',
  'surfaces.section': 'Geltungsbereich',
  'surfaces.main': 'Hauptfenster des Editors',
  'surfaces.hint': 'Plugin-Fenster mit eingebettetem Host-Editor erscheinen hier automatisch.',
  'effects.section': 'Effekte',
  'explosion.enable': 'Cursor-Explosionen',
  'explosion.preset': 'Voreinstellung',
  'preset.particle': 'Partikel',
  'preset.lightning': 'Blitz',
  'preset.coin': 'Münze',
  'preset.confetti': 'Konfetti',
  'shake.enable': 'Bildschirmwackeln',
  'shake.intensity': 'Stärke',
  'shake.level.light': 'Leicht',
  'shake.level.medium': 'Mittel',
  'shake.level.heavy': 'Stark',
  'combo.enable': 'Combo-Zähler',
  'combo.timeout': 'Zeitlimit',
  'combo.timeout.short': 'Kurz',
  'combo.timeout.medium': 'Mittel',
  'combo.timeout.long': 'Lang',
  'combo.showExclamation': 'Ausrufe',
  'combo.precisionInput': 'Präzise Eingabe',
  'combo.precisionInput.hint': 'Nur Änderungen zählen, die das Dokument nicht kürzen.',
  'demo.section': 'Ausprobieren',
  'demo.hint': 'Hier tippen — die Einstellungen oben wirken sofort, unabhängig von den Schaltern.',
  'demo.sample': 'Tippe etwas und sieh die Funken fliegen.',
  'demo.unavailable': 'Die Live-Vorschau benötigt eine neuere Version von note.md.',
  saveFailed: 'Einstellungen konnten nicht gespeichert werden',
}

export const CATALOGS = { en, zh, ja, de } as const

let current: keyof typeof CATALOGS = 'en'

export function setLocale(locale: string): void {
  const base = (locale.split('-')[0] ?? 'en') as keyof typeof CATALOGS
  current = base in CATALOGS ? base : 'en'
}

export function t(key: MessageKey): string {
  return CATALOGS[current][key] ?? CATALOGS.en[key]
}
