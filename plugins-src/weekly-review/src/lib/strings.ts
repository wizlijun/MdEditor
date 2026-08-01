// src/lib/strings.ts — self-contained i18n for the weekly-review plugin.
//
// A plugin window can't import the host's i18n store, so this mirrors its
// shape (src/lib/i18n/store.svelte.ts) in miniature: a MessageKey union, one
// catalog per locale, and a `t()` that falls back to English. Language is
// chosen from `notemd.locale` at startup via `setLocale`; see App.svelte.

export type Locale = 'en' | 'zh' | 'ja' | 'de'

export type MessageKey =
  | 'title'
  | 'thisWeek'
  | 'rebuild'
  | 'legend.review'
  | 'legend.today'
  | 'legend.past'
  | 'legend.future'
  | 'legend.diary'
  | 'legend.note'
  | 'empty.noVault'
  | 'empty.noData'
  | 'tip.review'
  | 'tip.none'
  | 'tip.future'
  | 'tip.diary'
  | 'tip.note'
  | 'month.suffix'
  | 'nav.prevYear'
  | 'nav.nextYear'
  | 'dow.mon'
  | 'dow.tue'
  | 'dow.wed'
  | 'dow.thu'
  | 'dow.fri'
  | 'dow.sat'
  | 'dow.sun'

type Catalog = Record<MessageKey, string>

const en: Catalog = {
  title: 'Weekly Review',
  thisWeek: 'This week',
  rebuild: 'Rebuild',
  'legend.review': 'Has review (click to open)',
  'legend.today': 'This week',
  'legend.past': 'Past',
  'legend.future': 'Upcoming',
  'legend.diary': 'Has diary (click the number)',
  'legend.note': 'Has note (click the star)',
  'empty.noVault': 'Configure a Vault to see your weekly reviews.',
  'empty.noData': 'No weekly reviews yet. Add files to the weekly-review/ folder.',
  'tip.review': 'has review — click to open',
  'tip.none': 'no review',
  'tip.future': 'upcoming',
  'tip.diary': 'open diary',
  'tip.note': 'open note outline',
  'month.suffix': '',
  'nav.prevYear': 'previous year',
  'nav.nextYear': 'next year',
  'dow.mon': 'M',
  'dow.tue': 'T',
  'dow.wed': 'W',
  'dow.thu': 'T',
  'dow.fri': 'F',
  'dow.sat': 'S',
  'dow.sun': 'S',
}

const zh: Catalog = {
  title: '周检视',
  thisWeek: '本周',
  rebuild: '重构',
  'legend.review': '有周报(点击打开)',
  'legend.today': '本周',
  'legend.past': '已过去',
  'legend.future': '未来',
  'legend.diary': '有日记(点数字)',
  'legend.note': '有笔记(点图标)',
  'empty.noVault': '请先配置 Vault,才能查看每周检视。',
  'empty.noData': '还没有周报。把文件放进 weekly-review/ 目录。',
  'tip.review': '有周报 · 点击打开',
  'tip.none': '无',
  'tip.future': '未来',
  'tip.diary': '打开日记',
  'tip.note': '打开笔记大纲',
  'month.suffix': '月',
  'nav.prevYear': '上一年',
  'nav.nextYear': '下一年',
  'dow.mon': '一',
  'dow.tue': '二',
  'dow.wed': '三',
  'dow.thu': '四',
  'dow.fri': '五',
  'dow.sat': '六',
  'dow.sun': '日',
}

const ja: Catalog = {
  title: 'ウィークリーレビュー',
  thisWeek: '今週',
  rebuild: '再構築',
  'legend.review': 'レビューあり(クリックで開く)',
  'legend.today': '今週',
  'legend.past': '過去',
  'legend.future': '今後',
  'legend.diary': '日記あり(数字をクリック)',
  'legend.note': 'ノートあり(星をクリック)',
  'empty.noVault': 'Vault を設定するとレビューが表示されます。',
  'empty.noData': 'まだレビューがありません。weekly-review/ に追加してください。',
  'tip.review': 'レビューあり · クリックで開く',
  'tip.none': 'なし',
  'tip.future': '今後',
  'tip.diary': '日記を開く',
  'tip.note': 'ノートの概要を開く',
  'month.suffix': '月',
  'nav.prevYear': '前年',
  'nav.nextYear': '翌年',
  'dow.mon': '月',
  'dow.tue': '火',
  'dow.wed': '水',
  'dow.thu': '木',
  'dow.fri': '金',
  'dow.sat': '土',
  'dow.sun': '日',
}

const de: Catalog = {
  title: 'Wochenrückblick',
  thisWeek: 'Diese Woche',
  rebuild: 'Neu aufbauen',
  'legend.review': 'Rückblick vorhanden (zum Öffnen klicken)',
  'legend.today': 'Diese Woche',
  'legend.past': 'Vergangen',
  'legend.future': 'Bevorstehend',
  'legend.diary': 'Tagebuch (Zahl anklicken)',
  'legend.note': 'Notiz (Stern anklicken)',
  'empty.noVault': 'Konfiguriere ein Vault, um deine Rückblicke zu sehen.',
  'empty.noData': 'Noch keine Rückblicke. Lege Dateien im Ordner weekly-review/ ab.',
  'tip.review': 'Rückblick vorhanden · zum Öffnen klicken',
  'tip.none': 'keiner',
  'tip.future': 'bevorstehend',
  'tip.diary': 'Tagebuch öffnen',
  'tip.note': 'Notiz-Gliederung öffnen',
  'month.suffix': '',
  'nav.prevYear': 'Vorheriges Jahr',
  'nav.nextYear': 'Nächstes Jahr',
  'dow.mon': 'M',
  'dow.tue': 'D',
  'dow.wed': 'M',
  'dow.thu': 'D',
  'dow.fri': 'F',
  'dow.sat': 'S',
  'dow.sun': 'S',
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

// Exported for tests only (catalog completeness / placeholder parity checks).
export const CATALOGS: Record<Locale, Catalog> = registry
export const LOCALES: Locale[] = ['en', 'zh', 'ja', 'de']
