const NONLOCAL_URL_SOURCE = '(?:(?:[a-z][a-z0-9+.-]*:)|//)[^\\s"\'<>)]+'
const INLINE_NONLOCAL_IMAGE_RE = new RegExp(
  `(!\\[[^\\]\\r\\n]*\\]\\(\\s*<?)(${NONLOCAL_URL_SOURCE})(>?)`,
  'gi',
)
const HTML_TAG_RE = /<[^>]+>/g
const MEDIA_TAG_RE = /^<(?:img|source|video|audio)\b/i
const MEDIA_SOURCE_ATTR_RE = /(\b(?:src|srcset|poster)\s*=\s*)(?:"([^"]*)"|'([^']*)'|([^\s>]+))/gi

let guardSequence = 0

function normalizedReference(value: string): string {
  return value.trim().replace(/\s+/g, ' ').toLowerCase()
}

/**
 * Reversibly replaces passive remote image/media sources before Markdown is
 * parsed by Moraya. The placeholder is a non-networking data URL and every
 * callback restores the exact original bytes before they reach Canvas state.
 */
export class CanvasMarkdownResourceGuard {
  private readonly prefix = `data:image/gif,notemd-canvas-remote-${++guardSequence}-`
  private nextId = 0
  private originalToPlaceholder = new Map<string, string>()
  private placeholderToOriginal = new Map<string, string>()

  shield(markdown: string): string {
    const imageReferences = new Set<string>()
    for (const match of markdown.matchAll(/!\[([^\]\r\n]*)\]\[([^\]\r\n]*)\]/g)) {
      imageReferences.add(normalizedReference(match[2] || match[1]))
    }

    let shielded = markdown.replace(INLINE_NONLOCAL_IMAGE_RE, (_all, prefix: string, url: string, suffix: string) => (
      `${prefix}${this.placeholderFor(url)}${suffix}`
    ))
    shielded = shielded.replace(HTML_TAG_RE, (tag) => {
      if (!MEDIA_TAG_RE.test(tag)) return tag
      return tag.replace(MEDIA_SOURCE_ATTR_RE, (_all, prefix: string, double: string, single: string, bare: string) => {
        const source = double ?? single ?? bare ?? ''
        const quote = double !== undefined ? '"' : single !== undefined ? "'" : ''
        return `${prefix}${quote}${this.placeholderFor(source)}${quote}`
      })
    })
    if (imageReferences.size > 0) {
      shielded = shielded.replace(
        new RegExp(`^(\\s{0,3}\\[([^\\]]+)\\]:\\s*<?)(${NONLOCAL_URL_SOURCE})(>?)`, 'gim'),
        (all, prefix: string, label: string, url: string, suffix: string) => (
          imageReferences.has(normalizedReference(label))
            ? `${prefix}${this.placeholderFor(url)}${suffix}`
            : all
        ),
      )
    }
    return shielded
  }

  restore(markdown: string): string {
    let restored = markdown
    for (const [placeholder, original] of this.placeholderToOriginal) {
      restored = restored.replaceAll(placeholder, original)
    }
    return restored
  }

  private placeholderFor(original: string): string {
    const existing = this.originalToPlaceholder.get(original)
    if (existing) return existing
    const placeholder = `${this.prefix}${++this.nextId}`
    this.originalToPlaceholder.set(original, placeholder)
    this.placeholderToOriginal.set(placeholder, original)
    return placeholder
  }
}

export function containsRemoteMediaHtml(html: string): boolean {
  return /<(?:img|source|video|audio)\b/i.test(html)
}
