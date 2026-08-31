import type { PluginMarketI18n } from './types'

export interface PluginTextSource {
  id: string
  name?: string | null
  description?: string | null
  i18n?: PluginMarketI18n | null | unknown
}

function nonBlank(value: unknown): string | null {
  return typeof value === 'string' && value.trim() !== '' ? value.trim() : null
}

function localizedField(
  source: PluginTextSource,
  locale: string,
  field: 'name' | 'description',
): string | null {
  if (!source.i18n || typeof source.i18n !== 'object' || Array.isArray(source.i18n)) return null
  const catalogs = source.i18n as Record<string, unknown>
  const normalized = locale.trim().toLowerCase()
  const candidates = normalized.includes('-')
    ? [normalized, normalized.split('-')[0]]
    : [normalized]
  for (const candidate of candidates) {
    const catalog = catalogs[candidate]
    if (!catalog || typeof catalog !== 'object' || Array.isArray(catalog)) continue
    const value = nonBlank((catalog as Record<string, unknown>)[field])
    if (value) return value
  }
  return null
}

/** Localized product name, retaining the English name for recognition. */
export function localizedPluginName(source: PluginTextSource, locale: string): string {
  const englishName = nonBlank(source.name) ?? source.id
  if (locale.trim().toLowerCase().split('-')[0] === 'en') return englishName
  const localizedName = localizedField(source, locale, 'name')
  if (!localizedName || localizedName.localeCompare(englishName, undefined, { sensitivity: 'accent' }) === 0) {
    return englishName
  }
  // Keep the English product identity beside names written in non-Western
  // scripts. Latin-script localizations (for example German) stay concise.
  const hasNonLatinLetter = Array.from(localizedName)
    .some((char) => /\p{Letter}/u.test(char) && !/\p{Script=Latin}/u.test(char))
  return hasNonLatinLetter ? `${localizedName}（${englishName}）` : localizedName
}

/** Localized description with a per-field fallback to the English base. */
export function localizedPluginDescription(source: PluginTextSource, locale: string): string | null {
  return localizedField(source, locale, 'description') ?? nonBlank(source.description)
}
